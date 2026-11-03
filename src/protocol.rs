//! 클라이언트 하나와의 대화: 객체 표와 요청 처리.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

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
}

impl Object {
    pub fn interface(self) -> &'static str {
        match self {
            Object::Display => "wl_display",
            Object::Registry => "wl_registry",
            Object::Callback => "wl_callback",
            Object::Compositor => "wl_compositor",
        }
    }
}

/// 컴포지터가 클라이언트에게 "이런 기능이 있다"고 알리는 전역 객체.
pub struct Global {
    pub name: u32,
    pub interface: &'static str,
    pub version: u32,
}

pub const GLOBALS: &[Global] = &[Global {
    name: 1,
    interface: "wl_compositor",
    version: 4,
}];

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
            ProtocolError::Wire(_) => (1, INVALID_METHOD),
        }
    }
}

/// 연결된 클라이언트 하나의 상태: 객체 표와 보낼 바이트.
pub struct Client {
    pub objects: HashMap<u32, Object>,
    pub outgoing: Vec<u8>,
    pub next_serial: u32,
}

impl Default for Client {
    fn default() -> Client {
        Client::new()
    }
}

impl Client {
    pub fn new() -> Client {
        let mut objects = HashMap::new();
        // 객체 1번은 언제나 wl_display다. 접속하는 순간부터 있다.
        objects.insert(1, Object::Display);
        Client {
            objects,
            outgoing: Vec::new(),
            next_serial: 1,
        }
    }

    pub fn send(&mut self, message: Vec<u8>) {
        self.outgoing.extend_from_slice(&message);
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
    pub fn receive(&mut self, bytes: &[u8]) -> Result<usize, ProtocolError> {
        let mut used = 0;
        while let Some((header, body)) = split_message(&bytes[used..])? {
            self.handle(header, body)?;
            used += usize::from(header.size);
        }
        Ok(used)
    }

    fn handle(&mut self, header: Header, body: &[u8]) -> Result<(), ProtocolError> {
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
                let serial = self.next_serial;
                self.next_serial += 1;
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
                    _ => return Err(ProtocolError::UnknownGlobal(name)),
                };
                self.create(id, object)?;
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
        let mut client = Client::new();
        let sync = MessageBuilder::new(1, 0).uint(5).finish();
        assert_eq!(client.receive(&sync), Ok(12));
        let events = events(&mut client);
        assert_eq!(events.len(), 2);
        assert_eq!((events[0].0.object, events[0].0.opcode), (5, 0)); // callback.done
        assert_eq!((events[1].0.object, events[1].0.opcode), (1, 1)); // display.delete_id
        assert_eq!(Args::new(&events[1].1).uint(), Ok(5));
        assert!(!client.objects.contains_key(&5));
    }

    #[test]
    fn registry_announces_globals_and_bind_creates_object() {
        let mut client = Client::new();
        client
            .receive(&MessageBuilder::new(1, 1).uint(2).finish())
            .unwrap();
        let events = events(&mut client);
        assert_eq!(events.len(), GLOBALS.len());
        let bind = MessageBuilder::new(2, 0)
            .uint(1)
            .string("wl_compositor")
            .uint(4)
            .uint(3)
            .finish();
        client.receive(&bind).unwrap();
        assert_eq!(client.objects.get(&3), Some(&Object::Compositor));
    }

    #[test]
    fn partial_messages_are_left_for_later() {
        let mut client = Client::new();
        let mut bytes = MessageBuilder::new(1, 0).uint(5).finish();
        bytes.extend_from_slice(&MessageBuilder::new(1, 0).uint(6).finish()[..4]);
        assert_eq!(client.receive(&bytes), Ok(12));
    }

    #[test]
    fn protocol_errors() {
        let mut client = Client::new();
        assert_eq!(
            client.receive(&MessageBuilder::new(9, 0).finish()),
            Err(ProtocolError::UnknownObject(9))
        );
        assert_eq!(
            client.receive(&MessageBuilder::new(1, 1).uint(1).finish()),
            Err(ProtocolError::IdInUse(1))
        );
        assert_eq!(
            client.receive(&MessageBuilder::new(1, 7).finish()),
            Err(ProtocolError::InvalidMethod {
                object: 1,
                opcode: 7
            })
        );
    }
}
