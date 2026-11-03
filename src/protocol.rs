//! 클라이언트 하나와의 대화: 객체 표와 요청 처리.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::os::fd::OwnedFd;
use std::rc::Rc;

use memmap2::{Mmap, MmapOptions};

use crate::layout::{Desktop, Rect};
use crate::wire::{Args, Header, MessageBuilder, WireError, split_message};
use crate::xdg::{Toplevel, XdgSurface};

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
    Surface,
    Region,
    WmBase,
    Positioner,
    XdgSurface,
    Toplevel,
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
            Object::Surface => "wl_surface",
            Object::Region => "wl_region",
            Object::WmBase => "xdg_wm_base",
            Object::Positioner => "xdg_positioner",
            Object::XdgSurface => "xdg_surface",
            Object::Toplevel => "xdg_toplevel",
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
            Object::Surface => &[
                "destroy",
                "attach",
                "damage",
                "frame",
                "set_opaque_region",
                "set_input_region",
                "commit",
                "set_buffer_transform",
                "set_buffer_scale",
                "damage_buffer",
            ],
            Object::Region => &["destroy", "add", "subtract"],
            Object::WmBase => &["destroy", "create_positioner", "get_xdg_surface", "pong"],
            Object::Positioner => &[
                "destroy",
                "set_size",
                "set_anchor_rect",
                "set_anchor",
                "set_gravity",
                "set_constraint_adjustment",
                "set_offset",
            ],
            Object::XdgSurface => &[
                "destroy",
                "get_toplevel",
                "get_popup",
                "set_window_geometry",
                "ack_configure",
            ],
            Object::Toplevel => &[
                "destroy",
                "set_parent",
                "set_title",
                "set_app_id",
                "show_window_menu",
                "move",
                "resize",
                "set_max_size",
                "set_min_size",
                "set_maximized",
                "unset_maximized",
                "set_fullscreen",
                "unset_fullscreen",
                "set_minimized",
            ],
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
            Object::Surface => &["enter", "leave"],
            Object::Region => &[],
            Object::WmBase => &["ping"],
            Object::Positioner => &[],
            Object::XdgSurface => &["configure"],
            Object::Toplevel => &["configure", "close"],
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
    Global {
        name: 3,
        interface: "xdg_wm_base",
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

/// 컴포지터가 가진 그림 한 장. 버퍼에서 복사해 온 것이라 클라이언트가 버퍼를 고쳐도 바뀌지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// 한 줄씩 위에서 아래로, 픽셀 하나는 0xAARRGGBB.
    pub pixels: Vec<u32>,
}

impl Image {
    fn copy_from(buffer: &Buffer) -> Image {
        let mut pixels = Vec::with_capacity(buffer.width as usize * buffer.height as usize);
        for y in 0..buffer.height {
            for x in 0..buffer.width {
                let pixel = buffer.pixel(x, y);
                // xrgb8888의 위 8비트는 쓰레기일 수 있다. 불투명으로 못 박는다.
                if buffer.format == FORMAT_XRGB8888 {
                    pixels.push(pixel | 0xff00_0000);
                } else {
                    pixels.push(pixel);
                }
            }
        }
        Image {
            width: buffer.width,
            height: buffer.height,
            pixels,
        }
    }
}

/// 클라이언트가 commit하기 전까지 모아 두는 변경 사항.
#[derive(Debug, Default)]
pub struct PendingState {
    /// `None`: 이번에 attach하지 않았다. `Some(None)`: 버퍼를 떼어 냈다. `Some(Some(id))`: 이 버퍼를 붙였다.
    pub buffer: Option<Option<u32>>,
    pub damaged: bool,
    pub frame_callbacks: Vec<u32>,
}

/// 표면 하나: 대기 중인 변경과, 마지막으로 commit된 그림.
#[derive(Debug, Default)]
pub struct Surface {
    pub pending: PendingState,
    pub image: Option<Image>,
    /// commit됐지만 아직 "그렸다"고 알리지 않은 frame 콜백들.
    pub frame_callbacks: Vec<u32>,
    pub commits: u32,
    /// 이 표면에 붙은 xdg_surface. 이것이 있어야 창이 될 수 있다.
    pub xdg_surface: Option<u32>,
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
    /// 화면과 그 위의 창들(1부에서 만든 그것).
    pub desktop: Desktop,
    /// 창 id → 그 창에 그림을 대는 (클라이언트, 표면).
    pub windows: HashMap<u32, (u32, u32)>,
    /// 지난번에 그린 뒤로 바뀐 것이 있다.
    pub dirty: bool,
}

impl Default for Shared {
    fn default() -> Shared {
        Shared::new(Rect::new(0, 0, 800, 480))
    }
}

impl Shared {
    pub fn new(output: Rect) -> Shared {
        Shared {
            next_serial: 1,
            desktop: Desktop::new(output),
            windows: HashMap::new(),
            dirty: false,
        }
    }

    pub(crate) fn window_for(&self, client: u32, surface: u32) -> Option<u32> {
        self.windows
            .iter()
            .find(|(_, owner)| **owner == (client, surface))
            .map(|(window, _)| *window)
    }

    /// 표면에 창이 없으면 하나 연다.
    pub(crate) fn show(&mut self, client: u32, surface: u32) {
        if self.window_for(client, surface).is_none() {
            let window = self
                .desktop
                .open(&format!("client {client} surface {surface}"));
            self.windows.insert(window, (client, surface));
        }
    }

    /// 표면의 창을 닫는다.
    pub(crate) fn hide(&mut self, client: u32, surface: u32) {
        if let Some(window) = self.window_for(client, surface) {
            self.desktop.close(window);
            self.windows.remove(&window);
            self.dirty = true;
        }
    }

    /// 연결이 끊긴 클라이언트의 창을 모두 닫는다.
    pub fn forget_client(&mut self, client: u32) {
        let surfaces: Vec<u32> = self
            .windows
            .values()
            .filter(|(owner, _)| *owner == client)
            .map(|(_, surface)| *surface)
            .collect();
        for surface in surfaces {
            self.hide(client, surface);
        }
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
    pub surfaces: HashMap<u32, Surface>,
    pub xdg_surfaces: HashMap<u32, XdgSurface>,
    pub toplevels: HashMap<u32, Toplevel>,
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
            surfaces: HashMap::new(),
            xdg_surfaces: HashMap::new(),
            toplevels: HashMap::new(),
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
    pub(crate) fn create(&mut self, id: u32, object: Object) -> Result<(), ProtocolError> {
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
                    "xdg_wm_base" => Object::WmBase,
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
            // wl_compositor.create_surface(id)
            (Object::Compositor, 0) => {
                let id = args.uint()?;
                self.create(id, Object::Surface)?;
                self.surfaces.insert(id, Surface::default());
            }
            // wl_compositor.create_region(id): 영역은 받아 두기만 하고 쓰지 않는다.
            (Object::Compositor, 1) => {
                let id = args.uint()?;
                self.create(id, Object::Region)?;
            }
            (Object::Region, 0) => self.destroy(header.object),
            (Object::Region, 1 | 2) => {}
            (Object::Surface, opcode) => {
                self.handle_surface(header.object, opcode, args, shared)?;
            }
            (
                Object::WmBase | Object::Positioner | Object::XdgSurface | Object::Toplevel,
                opcode,
            ) => {
                self.handle_xdg(object, header.object, opcode, args, shared)?;
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

    fn handle_surface(
        &mut self,
        id: u32,
        opcode: u16,
        mut args: Args<'_>,
        shared: &mut Shared,
    ) -> Result<(), ProtocolError> {
        match opcode {
            // destroy
            0 => {
                self.surfaces.remove(&id);
                shared.hide(self.id, id);
                self.destroy(id);
            }
            // attach(buffer, x, y): 0번 객체는 "버퍼 없음"
            1 => {
                let buffer = args.uint()?;
                let _offset = (args.int()?, args.int()?);
                let surface = self.surfaces.get_mut(&id).expect("surface exists");
                surface.pending.buffer = Some(if buffer == 0 { None } else { Some(buffer) });
            }
            // damage / damage_buffer: 어느 부분이 바뀌었는지. 이 컴포지터는 통째로 다시 그린다.
            2 | 9 => {
                self.surfaces
                    .get_mut(&id)
                    .expect("surface exists")
                    .pending
                    .damaged = true;
            }
            // frame(callback): 다음 그림을 그리기 좋은 때가 되면 알려 달라
            3 => {
                let callback = args.uint()?;
                self.create(callback, Object::Callback)?;
                let surface = self.surfaces.get_mut(&id).expect("surface exists");
                surface.pending.frame_callbacks.push(callback);
            }
            // set_opaque_region, set_input_region, set_buffer_transform, set_buffer_scale: 무시한다
            4 | 5 | 7 | 8 => {}
            // commit: 모아 둔 변경을 한꺼번에 적용한다
            6 => self.commit(id, shared)?,
            _ => return Err(ProtocolError::InvalidMethod { object: id, opcode }),
        }
        Ok(())
    }

    fn commit(&mut self, id: u32, shared: &mut Shared) -> Result<(), ProtocolError> {
        let surface = self.surfaces.get_mut(&id).expect("surface exists");
        // 창이 되려는 표면은, 크기를 받아(configure) 확인(ack)하기 전에 버퍼를 붙일 수 없다.
        if let Some(xdg_id) = surface.xdg_surface
            && matches!(surface.pending.buffer, Some(Some(_)))
            && !self.xdg_surfaces[&xdg_id].acked
        {
            return Err(ProtocolError::Object {
                object: xdg_id,
                code: 3, // xdg_surface.error.unconfigured_buffer
                message: "buffer attached before acking the first configure".to_string(),
            });
        }
        let pending = std::mem::take(&mut surface.pending);
        let mut released = None;
        match pending.buffer {
            Some(Some(buffer_id)) => {
                if let Some(buffer) = self.buffers.get(&buffer_id) {
                    surface.image = Some(Image::copy_from(buffer));
                    released = Some(buffer_id);
                }
            }
            Some(None) => surface.image = None,
            None => {}
        }
        surface.frame_callbacks.extend(pending.frame_callbacks);
        surface.commits += 1;
        shared.dirty = true;
        if self.log {
            let size = surface
                .image
                .as_ref()
                .map(|image| format!("{}x{}", image.width, image.height))
                .unwrap_or_else(|| "no buffer".to_string());
            println!(
                "[{}]      surface#{id} commit {}: {size}",
                self.id, surface.commits
            );
        }
        // 픽셀을 복사해 왔으니 클라이언트는 그 버퍼를 다시 써도 된다.
        if let Some(buffer_id) = released {
            self.send(MessageBuilder::new(buffer_id, 0).finish());
        }
        Ok(())
    }

    /// 화면을 한 번 그렸다고 치고, 기다리던 frame 콜백에 모두 답한다.
    pub fn frame_done(&mut self, time_ms: u32) {
        let mut callbacks = Vec::new();
        for surface in self.surfaces.values_mut() {
            callbacks.append(&mut surface.frame_callbacks);
        }
        for callback in callbacks {
            self.send(MessageBuilder::new(callback, 0).uint(time_ms).finish());
            self.destroy(callback);
        }
    }

    /// 객체를 지우고, 클라이언트에게 그 id를 다시 써도 된다고 알린다(wl_display.delete_id).
    pub(crate) fn destroy(&mut self, id: u32) {
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
        assert_eq!(client.receive(&sync, &mut Shared::default()), Ok(12));
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
                &mut Shared::default(),
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
        client.receive(&bind, &mut Shared::default()).unwrap();
        assert_eq!(client.objects.get(&3), Some(&Object::Compositor));
    }

    #[test]
    fn partial_messages_are_left_for_later() {
        let mut client = Client::new(1);
        let mut bytes = MessageBuilder::new(1, 0).uint(5).finish();
        bytes.extend_from_slice(&MessageBuilder::new(1, 0).uint(6).finish()[..4]);
        assert_eq!(client.receive(&bytes, &mut Shared::default()), Ok(12));
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
        let mut shared = Shared::default();
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
        let mut shared = Shared::default();
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
    fn surface_changes_wait_for_commit() {
        let mut shared = Shared::default();
        let mut client = client_with_shm(&mut shared);
        client.fds.push_back(memfd(16, 0x00abcdef));
        let mut bytes = MessageBuilder::new(3, 0).uint(4).int(16).finish();
        bytes.extend(
            MessageBuilder::new(4, 0)
                .uint(5)
                .int(0)
                .int(2)
                .int(2)
                .int(8)
                .uint(FORMAT_XRGB8888)
                .finish(),
        );
        // wl_compositor를 id 6으로 bind하고 표면 7을 만든다.
        bytes.extend(
            MessageBuilder::new(2, 0)
                .uint(1)
                .string("wl_compositor")
                .uint(4)
                .uint(6)
                .finish(),
        );
        bytes.extend(MessageBuilder::new(6, 0).uint(7).finish());
        bytes.extend(MessageBuilder::new(7, 1).uint(5).int(0).int(0).finish());
        client.receive(&bytes, &mut shared).unwrap();
        client.outgoing.clear();
        assert_eq!(
            client.surfaces[&7].image, None,
            "attach alone changes nothing"
        );

        client
            .receive(&MessageBuilder::new(7, 6).finish(), &mut shared)
            .unwrap();
        let image = client.surfaces[&7].image.as_ref().unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.pixels[0], 0xffabcdef, "xrgb is made opaque");
        let events = events(&mut client);
        assert_eq!((events[0].0.object, events[0].0.opcode), (5, 0)); // buffer.release
    }

    #[test]
    fn protocol_errors() {
        let mut client = Client::new(1);
        assert_eq!(
            client.receive(&MessageBuilder::new(9, 0).finish(), &mut Shared::default()),
            Err(ProtocolError::UnknownObject(9))
        );
        assert_eq!(
            client.receive(
                &MessageBuilder::new(1, 1).uint(1).finish(),
                &mut Shared::default()
            ),
            Err(ProtocolError::IdInUse(1))
        );
        assert_eq!(
            client.receive(&MessageBuilder::new(1, 7).finish(), &mut Shared::default()),
            Err(ProtocolError::InvalidMethod {
                object: 1,
                opcode: 7
            })
        );
    }
}
