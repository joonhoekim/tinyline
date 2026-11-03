//! 클라이언트 하나와의 대화: 객체 표와 요청 처리.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::os::fd::OwnedFd;
use std::rc::Rc;

use memmap2::{Mmap, MmapOptions};

use crate::wire::{Args, Header, MessageBuilder, WireError, split_message};

/// 클라이언트가 만든 객체 하나의 종류.
///
/// 클라이언트는 객체를 만들 때 번호(id)를 정해서 알려 주고, 그 뒤로는 번호로만 부른다.
/// 컴포지터는 번호마다 그것이 무슨 객체인지 적어 둬야 메시지를 알아들을 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Object {
    Display,
    Registry,
    Callback,
    Compositor,
    Shm,
    ShmPool,
    Buffer,
}

impl Object {
    pub fn interface(self) -> &'static str {
        match self {
            Object::Display => "wl_display",
            Object::Registry => "wl_registry",
            Object::Callback => "wl_callback",
            Object::Compositor => "wl_compositor",
            Object::Shm => "wl_shm",
            Object::ShmPool => "wl_shm_pool",
            Object::Buffer => "wl_buffer",
        }
    }

    /// 요청 번호의 이름. 로그를 사람이 읽을 수 있게 하려는 것뿐이다.
    pub fn request_name(self, opcode: u16) -> &'static str {
        let names: &[&str] = match self {
            Object::Display => &["sync", "get_registry"],
            Object::Registry => &["bind"],
            Object::Callback => &[],
            Object::Compositor => &["create_surface", "create_region"],
            Object::Shm => &["create_pool"],
            Object::ShmPool => &["create_buffer", "destroy", "resize"],
            Object::Buffer => &["destroy"],
        };
        names.get(usize::from(opcode)).copied().unwrap_or("?")
    }

    /// 이벤트 번호의 이름.
    pub fn event_name(self, opcode: u16) -> &'static str {
        let names: &[&str] = match self {
            Object::Display => &["error", "delete_id"],
            Object::Registry => &["global", "global_remove"],
            Object::Callback => &["done"],
            Object::Compositor => &[],
            Object::Shm => &["format"],
            Object::ShmPool => &[],
            Object::Buffer => &["release"],
        };
        names.get(usize::from(opcode)).copied().unwrap_or("?")
    }
}

/// 컴포지터가 클라이언트에게 "이런 기능이 있다"고 알리는 전역 객체.
pub struct Global {
    pub name: u32,
    pub interface: &'static str,
    pub version: u32,
}

pub const GLOBALS: &[Global] = &[
    Global {
        name: 1,
        interface: "wl_compositor",
        version: 4,
    },
    Global {
        name: 2,
        interface: "wl_shm",
        version: 1,
    },
];

/// wl_shm이 받아 주는 픽셀 형식. 둘 다 픽셀 하나가 4바이트다.
pub const FORMAT_ARGB8888: u32 = 0;
pub const FORMAT_XRGB8888: u32 = 1;

/// 클라이언트가 넘겨준 공유 메모리 한 덩어리.
pub struct Pool {
    file: File,
    map: Mmap,
}

impl Pool {
    fn map(file: File, size: usize) -> std::io::Result<Pool> {
        // SAFETY: 이 매핑은 읽기만 한다. 클라이언트가 같은 메모리를 언제든 고칠 수 있으므로
        // 읽은 값이 엉뚱할 수는 있지만, Rust의 메모리 규칙이 깨지지는 않는다 —
        // 우리는 이 메모리에서 바이트를 복사해 올 뿐, 그 안을 가리키는 참조를 오래 들고 있지 않는다.
        // (클라이언트가 파일을 줄여 버리면 읽다가 SIGBUS로 죽을 수 있다 — 실제 컴포지터는
        // 이 경우를 따로 막는다. 여기서는 다루지 않는다.)
        let map = unsafe { MmapOptions::new().len(size).map(&file)? };
        Ok(Pool { file, map })
    }
}

/// 공유 메모리 안의 그림 한 장: 어디서부터, 몇 픽셀 × 몇 줄, 한 줄이 몇 바이트인지.
pub struct Buffer {
    pub pool: Rc<Pool>,
    pub offset: usize,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: u32,
}

impl Buffer {
    /// (x, y) 픽셀 하나. 메모리에는 작은 쪽 바이트부터(파랑, 초록, 빨강, 투명도) 놓여 있다.
    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        let start = self.offset + y as usize * self.stride + x as usize * 4;
        let bytes: [u8; 4] = self.pool.map[start..start + 4].try_into().expect("4 bytes");
        u32::from_le_bytes(bytes)
    }
}

/// 클라이언트가 말한 버퍼가 풀 안에 들어가는지 확인한다. 말이 안 되면 `None`.
fn check_buffer(
    pool: &Rc<Pool>,
    offset: i32,
    width: i32,
    height: i32,
    stride: i32,
    format: u32,
) -> Option<Buffer> {
    if format != FORMAT_ARGB8888 && format != FORMAT_XRGB8888 {
        return None;
    }
    let offset = usize::try_from(offset).ok()?;
    let width = u32::try_from(width).ok().filter(|w| *w > 0)?;
    let height = u32::try_from(height).ok().filter(|h| *h > 0)?;
    let stride = usize::try_from(stride).ok()?;
    if stride < width as usize * 4 {
        return None;
    }
    let end = offset.checked_add(stride.checked_mul(height as usize)?)?;
    if end > pool.map.len() {
        return None;
    }
    Some(Buffer {
        pool: Rc::clone(pool),
        offset,
        width,
        height,
        stride,
        format,
    })
}

/// 클라이언트가 규칙을 어겼을 때. 클라이언트에게 `wl_display.error`로 알리고 연결을 끊는다.
#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    /// 없는 객체에 메시지를 보냈다.
    UnknownObject(u32),
    /// 객체는 있는데 그런 opcode가 없다(또는 아직 구현하지 않았다).
    InvalidMethod { object: u32, opcode: u16 },
    /// 이미 쓰고 있는 id로 새 객체를 만들려 했다.
    IdInUse(u32),
    /// 알리지 않은 전역 객체를 bind하려 했다.
    UnknownGlobal(u32),
    /// 메시지 자체가 망가졌다.
    Wire(WireError),
    /// fd를 받아야 하는 요청인데 함께 온 fd가 없다.
    MissingFd,
    /// 특정 인터페이스가 정한 에러(코드는 그 인터페이스의 error enum).
    Object {
        object: u32,
        code: u32,
        message: String,
    },
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolError::UnknownObject(id) => write!(f, "object {id} does not exist"),
            ProtocolError::InvalidMethod { object, opcode } => {
                write!(f, "object {object} has no request {opcode}")
            }
            ProtocolError::IdInUse(id) => write!(f, "id {id} is already in use"),
            ProtocolError::UnknownGlobal(name) => write!(f, "no global named {name}"),
            ProtocolError::Wire(error) => write!(f, "malformed message: {error}"),
            ProtocolError::MissingFd => write!(f, "expected a file descriptor"),
            ProtocolError::Object { message, .. } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// `?`가 `WireError`를 `ProtocolError`로 바꿔 올려 보낼 수 있게 한다.
impl From<WireError> for ProtocolError {
    fn from(error: WireError) -> ProtocolError {
        ProtocolError::Wire(error)
    }
}

impl ProtocolError {
    /// `wl_display.error`의 (문제의 객체, 에러 코드). 코드는 wl_display의 error enum을 따른다.
    pub fn object_and_code(&self) -> (u32, u32) {
        const INVALID_OBJECT: u32 = 0;
        const INVALID_METHOD: u32 = 1;
        const IMPLEMENTATION: u32 = 3;
        match self {
            ProtocolError::UnknownObject(id) => (*id, INVALID_OBJECT),
            ProtocolError::InvalidMethod { object, .. } => (*object, INVALID_METHOD),
            ProtocolError::IdInUse(_) | ProtocolError::UnknownGlobal(_) => (1, IMPLEMENTATION),
            ProtocolError::Wire(_) | ProtocolError::MissingFd => (1, INVALID_METHOD),
            ProtocolError::Object { object, code, .. } => (*object, *code),
        }
    }
}

/// 모든 클라이언트가 함께 쓰는 컴포지터 쪽 상태.
pub struct Shared {
    next_serial: u32,
}

impl Default for Shared {
    fn default() -> Shared {
        Shared::new()
    }
}

impl Shared {
    pub fn new() -> Shared {
        Shared { next_serial: 1 }
    }

    /// 컴포지터 전체에서 하나씩 늘어나는 번호. 어느 클라이언트에 주든 겹치지 않는다.
    pub fn next_serial(&mut self) -> u32 {
        let serial = self.next_serial;
        self.next_serial += 1;
        serial
    }
}

/// 연결된 클라이언트 하나의 상태: 객체 표와 보낼 바이트.
pub struct Client {
    pub objects: HashMap<u32, Object>,
    pub outgoing: Vec<u8>,
    /// 로그에 찍을 클라이언트 번호.
    pub id: u32,
    /// 소켓으로 함께 온 파일 디스크립터. fd 인자가 있는 요청이 도착한 순서대로 하나씩 꺼내 간다.
    pub fds: VecDeque<OwnedFd>,
    pub pools: HashMap<u32, Rc<Pool>>,
    pub buffers: HashMap<u32, Buffer>,
    /// 켜 두면 주고받는 메시지를 한 줄씩 찍는다.
    pub log: bool,
}

impl Client {
    pub fn new(id: u32) -> Client {
        let mut objects = HashMap::new();
        // 객체 1번은 언제나 wl_display다. 접속하는 순간부터 있다.
        objects.insert(1, Object::Display);
        Client {
            objects,
            outgoing: Vec::new(),
            id,
            fds: VecDeque::new(),
            pools: HashMap::new(),
            buffers: HashMap::new(),
            log: false,
        }
    }

    pub fn send(&mut self, message: Vec<u8>) {
        if self.log
            && let Ok(Some((header, _))) = split_message(&message)
        {
            println!("[{}]   <- {}", self.id, self.describe(header, false));
        }
        self.outgoing.extend_from_slice(&message);
    }

    /// "wl_registry#2.global" 같은 한 줄 이름.
    fn describe(&self, header: Header, request: bool) -> String {
        match self.objects.get(&header.object) {
            Some(object) => {
                let name = if request {
                    object.request_name(header.opcode)
                } else {
                    object.event_name(header.opcode)
                };
                format!("{}#{}.{}", object.interface(), header.object, name)
            }
            None => format!("?#{} opcode {}", header.object, header.opcode),
        }
    }

    /// 새 객체를 표에 넣는다. 이미 있는 id면 에러.
    fn create(&mut self, id: u32, object: Object) -> Result<(), ProtocolError> {
        match self.objects.entry(id) {
            Entry::Occupied(_) => Err(ProtocolError::IdInUse(id)),
            Entry::Vacant(slot) => {
                slot.insert(object);
                Ok(())
            }
        }
    }

    /// 받은 바이트에서 완성된 메시지를 모두 처리하고, 처리한 바이트 수를 돌려준다.
    /// 끝에 덜 온 메시지가 남아 있으면 그 앞까지만 처리한다.
    pub fn receive(&mut self, bytes: &[u8], shared: &mut Shared) -> Result<usize, ProtocolError> {
        let mut used = 0;
        while let Some((header, body)) = split_message(&bytes[used..])? {
            if self.log {
                println!("[{}] -> {}", self.id, self.describe(header, true));
            }
            self.handle(header, body, shared)?;
            used += usize::from(header.size);
        }
        Ok(used)
    }

    fn handle(
        &mut self,
        header: Header,
        body: &[u8],
        shared: &mut Shared,
    ) -> Result<(), ProtocolError> {
        let object = *self
            .objects
            .get(&header.object)
            .ok_or(ProtocolError::UnknownObject(header.object))?;
        let mut args = Args::new(body);
        match (object, header.opcode) {
            // wl_display.sync(callback): 지금까지의 요청을 다 처리했다고 바로 답한다.
            (Object::Display, 0) => {
                let callback = args.uint()?;
                self.create(callback, Object::Callback)?;
                let serial = shared.next_serial();
                self.send(MessageBuilder::new(callback, 0).uint(serial).finish());
                self.destroy(callback);
            }
            // wl_display.get_registry(registry): 전역 객체 목록을 알려 준다.
            (Object::Display, 1) => {
                let registry = args.uint()?;
                self.create(registry, Object::Registry)?;
                for global in GLOBALS {
                    self.send(
                        MessageBuilder::new(registry, 0)
                            .uint(global.name)
                            .string(global.interface)
                            .uint(global.version)
                            .finish(),
                    );
                }
            }
            // wl_registry.bind(name, interface, version, id)
            (Object::Registry, 0) => {
                let name = args.uint()?;
                let interface = args.string()?;
                let _version = args.uint()?;
                let id = args.uint()?;
                let global = GLOBALS
                    .iter()
                    .find(|g| g.name == name && g.interface == interface)
                    .ok_or(ProtocolError::UnknownGlobal(name))?;
                let object = match global.interface {
                    "wl_compositor" => Object::Compositor,
                    "wl_shm" => Object::Shm,
                    _ => return Err(ProtocolError::UnknownGlobal(name)),
                };
                self.create(id, object)?;
                if object == Object::Shm {
                    for format in [FORMAT_ARGB8888, FORMAT_XRGB8888] {
                        self.send(MessageBuilder::new(id, 0).uint(format).finish());
                    }
                }
            }
            // wl_shm.create_pool(id, fd, size)
            (Object::Shm, 0) => {
                let id = args.uint()?;
                let fd = self.fds.pop_front().ok_or(ProtocolError::MissingFd)?;
                let size = args.int()?;
                let pool = usize::try_from(size)
                    .ok()
                    .filter(|size| *size > 0)
                    .and_then(|size| Pool::map(File::from(fd), size).ok())
                    .ok_or_else(|| ProtocolError::Object {
                        object: header.object,
                        code: 2, // wl_shm.error.invalid_fd
                        message: format!("cannot map {size} bytes"),
                    })?;
                self.create(id, Object::ShmPool)?;
                self.pools.insert(id, Rc::new(pool));
            }
            // wl_shm_pool.create_buffer(id, offset, width, height, stride, format)
            (Object::ShmPool, 0) => {
                let id = args.uint()?;
                let (offset, width, height) = (args.int()?, args.int()?, args.int()?);
                let (stride, format) = (args.int()?, args.uint()?);
                let pool = Rc::clone(&self.pools[&header.object]);
                let buffer = check_buffer(&pool, offset, width, height, stride, format)
                    .ok_or_else(|| ProtocolError::Object {
                        object: header.object,
                        code: 1, // wl_shm.error.invalid_stride
                        message: format!(
                            "bad buffer: offset {offset}, {width}x{height}, stride {stride}, format {format}"
                        ),
                    })?;
                self.create(id, Object::Buffer)?;
                if self.log {
                    println!(
                        "[{}]      buffer#{id}: {}x{}, first pixel {:08x}",
                        self.id,
                        buffer.width,
                        buffer.height,
                        buffer.pixel(0, 0)
                    );
                }
                self.buffers.insert(id, buffer);
            }
            // wl_shm_pool.destroy: 풀은 지워도, 이미 만든 버퍼는 메모리를 계속 쓴다(Rc).
            (Object::ShmPool, 1) => {
                self.pools.remove(&header.object);
                self.destroy(header.object);
            }
            // wl_shm_pool.resize(size): 같은 파일을 더 크게 다시 매핑한다.
            (Object::ShmPool, 2) => {
                let size = args.int()?;
                let old = &self.pools[&header.object];
                let bigger = usize::try_from(size)
                    .ok()
                    .filter(|size| *size >= old.map.len())
                    .and_then(|size| Pool::map(old.file.try_clone().ok()?, size).ok())
                    .ok_or_else(|| ProtocolError::Object {
                        object: header.object,
                        code: 2,
                        message: format!("cannot resize pool to {size} bytes"),
                    })?;
                self.pools.insert(header.object, Rc::new(bigger));
            }
            // wl_buffer.destroy
            (Object::Buffer, 0) => {
                self.buffers.remove(&header.object);
                self.destroy(header.object);
            }
            _ => {
                return Err(ProtocolError::InvalidMethod {
                    object: header.object,
                    opcode: header.opcode,
                });
            }
        }
        Ok(())
    }

    /// 객체를 지우고, 클라이언트에게 그 id를 다시 써도 된다고 알린다(wl_display.delete_id).
    fn destroy(&mut self, id: u32) {
        self.objects.remove(&id);
        self.send(MessageBuilder::new(1, 1).uint(id).finish());
    }

    /// 규칙 위반을 wl_display.error 이벤트로 만든다.
    pub fn send_error(&mut self, error: &ProtocolError) {
        let (object, code) = error.object_and_code();
        let message = error.to_string();
        self.send(
            MessageBuilder::new(1, 0)
                .uint(object)
                .uint(code)
                .string(&message)
                .finish(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(client: &mut Client) -> Vec<(Header, Vec<u8>)> {
        let bytes = std::mem::take(&mut client.outgoing);
        let mut rest = bytes.as_slice();
        let mut out = Vec::new();
        while let Some((header, body)) = split_message(rest).unwrap() {
            out.push((header, body.to_vec()));
            rest = &rest[usize::from(header.size)..];
        }
        out
    }

    #[test]
    fn sync_is_answered_and_its_id_released() {
        let mut client = Client::new(1);
        let sync = MessageBuilder::new(1, 0).uint(5).finish();
        assert_eq!(client.receive(&sync, &mut Shared::new()), Ok(12));
        let events = events(&mut client);
        assert_eq!(events.len(), 2);
        assert_eq!((events[0].0.object, events[0].0.opcode), (5, 0)); // callback.done
        assert_eq!((events[1].0.object, events[1].0.opcode), (1, 1)); // display.delete_id
        assert_eq!(Args::new(&events[1].1).uint(), Ok(5));
        assert!(!client.objects.contains_key(&5));
    }

    #[test]
    fn registry_announces_globals_and_bind_creates_object() {
        let mut client = Client::new(1);
        client
            .receive(
                &MessageBuilder::new(1, 1).uint(2).finish(),
                &mut Shared::new(),
            )
            .unwrap();
        let events = events(&mut client);
        assert_eq!(events.len(), GLOBALS.len());
        let bind = MessageBuilder::new(2, 0)
            .uint(1)
            .string("wl_compositor")
            .uint(4)
            .uint(3)
            .finish();
        client.receive(&bind, &mut Shared::new()).unwrap();
        assert_eq!(client.objects.get(&3), Some(&Object::Compositor));
    }

    #[test]
    fn partial_messages_are_left_for_later() {
        let mut client = Client::new(1);
        let mut bytes = MessageBuilder::new(1, 0).uint(5).finish();
        bytes.extend_from_slice(&MessageBuilder::new(1, 0).uint(6).finish()[..4]);
        assert_eq!(client.receive(&bytes, &mut Shared::new()), Ok(12));
    }

    /// 크기가 `size`인 메모리 파일을 만들어, 앞 4바이트에 `first`를 쓴다.
    fn memfd(size: usize, first: u32) -> OwnedFd {
        use std::io::Write;
        let fd = rustix::fs::memfd_create("test", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
        let mut file = File::from(fd);
        let mut bytes = vec![0u8; size];
        bytes[..4].copy_from_slice(&first.to_le_bytes());
        file.write_all(&bytes).unwrap();
        OwnedFd::from(file)
    }

    /// wl_shm을 id 3으로 bind한 클라이언트.
    fn client_with_shm(shared: &mut Shared) -> Client {
        let mut client = Client::new(1);
        let bind = MessageBuilder::new(2, 0)
            .uint(2)
            .string("wl_shm")
            .uint(1)
            .uint(3)
            .finish();
        let mut bytes = MessageBuilder::new(1, 1).uint(2).finish();
        bytes.extend(bind);
        client.receive(&bytes, shared).unwrap();
        client
    }

    #[test]
    fn shm_buffer_reads_client_memory() {
        let mut shared = Shared::new();
        let mut client = client_with_shm(&mut shared);
        client.fds.push_back(memfd(16 * 16 * 4, 0xff112233));
        let mut bytes = MessageBuilder::new(3, 0).uint(4).int(16 * 16 * 4).finish();
        bytes.extend(
            MessageBuilder::new(4, 0)
                .uint(5)
                .int(0)
                .int(16)
                .int(16)
                .int(64)
                .uint(FORMAT_ARGB8888)
                .finish(),
        );
        client.receive(&bytes, &mut shared).unwrap();
        assert_eq!(client.buffers[&5].pixel(0, 0), 0xff112233);
        assert_eq!(client.buffers[&5].pixel(15, 15), 0);
    }

    #[test]
    fn shm_rejects_missing_fd_and_oversized_buffers() {
        let mut shared = Shared::new();
        let mut client = client_with_shm(&mut shared);
        let create_pool = MessageBuilder::new(3, 0).uint(4).int(64).finish();
        assert_eq!(
            client.receive(&create_pool, &mut shared),
            Err(ProtocolError::MissingFd)
        );

        client.fds.push_back(memfd(64, 0));
        client.receive(&create_pool, &mut shared).unwrap();
        // 64바이트 풀에 4x4(=64바이트)는 들어가지만 4x5는 안 들어간다.
        let fits = MessageBuilder::new(4, 0)
            .uint(5)
            .int(0)
            .int(4)
            .int(4)
            .int(16)
            .uint(FORMAT_XRGB8888)
            .finish();
        assert!(client.receive(&fits, &mut shared).is_ok());
        let too_big = MessageBuilder::new(4, 0)
            .uint(6)
            .int(0)
            .int(4)
            .int(5)
            .int(16)
            .uint(FORMAT_XRGB8888)
            .finish();
        assert!(matches!(
            client.receive(&too_big, &mut shared),
            Err(ProtocolError::Object { code: 1, .. })
        ));
    }

    #[test]
    fn protocol_errors() {
        let mut client = Client::new(1);
        assert_eq!(
            client.receive(&MessageBuilder::new(9, 0).finish(), &mut Shared::new()),
            Err(ProtocolError::UnknownObject(9))
        );
        assert_eq!(
            client.receive(
                &MessageBuilder::new(1, 1).uint(1).finish(),
                &mut Shared::new()
            ),
            Err(ProtocolError::IdInUse(1))
        );
        assert_eq!(
            client.receive(&MessageBuilder::new(1, 7).finish(), &mut Shared::new()),
            Err(ProtocolError::InvalidMethod {
                object: 1,
                opcode: 7
            })
        );
    }
}
