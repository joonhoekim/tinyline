/// 화면 위의 사각형 하나. 왼쪽 위 모서리(x, y)와 너비·높이로 정한다.
///
/// 위치는 음수가 될 수 있다(창을 화면 왼쪽 밖으로 끌어낼 수 있다).
/// 크기는 음수가 될 수 없으므로 부호 없는 정수로 둔다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl Rect {
    fn new(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// 오른쪽 끝의 x 좌표(이 값 자체는 사각형에 들어가지 않는다).
    fn right(&self) -> i32 {
        self.x + i32::try_from(self.width).expect("width fits in i32")
    }

    /// 아래쪽 끝의 y 좌표(이 값 자체는 사각형에 들어가지 않는다).
    fn bottom(&self) -> i32 {
        self.y + i32::try_from(self.height).expect("height fits in i32")
    }

    fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

/// 컴포지터가 관리하는 창 하나.
#[derive(Debug)]
struct Window {
    id: u32,
    title: String,
    rect: Rect,
}

/// 길이 `total`을 `count`칸으로 나눴을 때 `index`번째 칸의 (시작 위치, 길이).
///
/// 나누어떨어지지 않으면 남는 길이를 앞쪽 칸부터 1씩 더 준다.
/// 그래서 칸들을 모두 이으면 빈틈도 겹침도 없이 정확히 `total`이 된다.
fn span(total: u32, count: u32, index: u32) -> (u32, u32) {
    let base = total / count;
    let extra = total % count;
    let length = if index < extra { base + 1 } else { base };
    let start = index * base + index.min(extra);
    (start, length)
}

/// `area`를 세로 기둥 `count`개로 나눈다. 왼쪽 기둥부터 차례로 돌려준다.
fn tile(area: Rect, count: usize) -> Vec<Rect> {
    let count = u32::try_from(count).expect("window count fits in u32");
    (0..count)
        .map(|index| {
            let (start, width) = span(area.width, count, index);
            let x = area.x + i32::try_from(start).expect("offset fits in i32");
            Rect::new(x, area.y, width, area.height)
        })
        .collect()
}

/// 화면 하나와 그 위의 창들.
struct Desktop {
    output: Rect,
    windows: Vec<Window>,
    next_id: u32,
    /// 키보드 입력을 받을 창. 창이 하나도 없으면 없다.
    focused: Option<u32>,
}

impl Desktop {
    fn new(output: Rect) -> Desktop {
        Desktop {
            output,
            windows: Vec::new(),
            next_id: 1,
            focused: None,
        }
    }

    /// 새 창을 열고 그 id를 돌려준다. 창을 열 때마다 화면을 다시 나누고,
    /// 새 창에 포커스를 준다.
    fn open(&mut self, title: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.windows.push(Window {
            id,
            title: title.to_string(),
            rect: self.output,
        });
        self.relayout();
        self.focused = Some(id);
        id
    }

    /// id에 해당하는 창의 목록 안 위치.
    fn index_of(&self, id: u32) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    /// id에 해당하는 창을 닫는다. 그런 창이 없었으면 false.
    ///
    /// 포커스된 창을 닫으면 그 자리를 이어받은 창(없으면 마지막 창)에 포커스를 넘긴다.
    fn close(&mut self, id: u32) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.windows.remove(index);
        if self.focused == Some(id) {
            let next = index.min(self.windows.len().saturating_sub(1));
            self.focused = self.windows.get(next).map(|w| w.id);
        }
        self.relayout();
        true
    }

    /// id에 해당하는 창에 포커스를 준다. 그런 창이 없으면 false.
    fn focus(&mut self, id: u32) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        self.focused = Some(id);
        true
    }

    /// 포커스를 오른쪽 창으로 옮긴다. 마지막 창에서는 첫 창으로 돌아간다.
    fn focus_next(&mut self) {
        if self.windows.is_empty() {
            return;
        }
        let next = match self.focused.and_then(|id| self.index_of(id)) {
            Some(index) => (index + 1) % self.windows.len(),
            None => 0,
        };
        self.focused = Some(self.windows[next].id);
    }

    fn focused_window(&self) -> Option<&Window> {
        let id = self.focused?;
        self.windows.iter().find(|w| w.id == id)
    }

    /// (x, y)를 클릭하면 맞는 창. 나중에 연 창이 위에 있으므로 뒤에서부터 찾는다.
    fn window_at(&self, x: i32, y: i32) -> Option<&Window> {
        self.windows.iter().rev().find(|w| w.rect.contains(x, y))
    }

    /// 지금 열린 창들에 화면을 고르게 나눠 준다.
    fn relayout(&mut self) {
        if self.windows.is_empty() {
            return;
        }
        let rects = tile(self.output, self.windows.len());
        for (window, rect) in self.windows.iter_mut().zip(rects) {
            window.rect = rect;
        }
    }

    /// 서로 겹치는 창의 쌍이 몇 개인지 센다.
    fn overlapping_pairs(&self) -> usize {
        let mut count = 0;
        for i in 0..self.windows.len() {
            for j in (i + 1)..self.windows.len() {
                if self.windows[i].rect.intersects(&self.windows[j].rect) {
                    count += 1;
                }
            }
        }
        count
    }

    fn print(&self) {
        println!("{} window(s):", self.windows.len());
        for window in &self.windows {
            let mark = if self.focused == Some(window.id) {
                '*'
            } else {
                ' '
            };
            println!(
                " {mark}#{} {:?} at {:?} ({} px)",
                window.id,
                window.title,
                window.rect,
                window.rect.area()
            );
        }
        println!("overlapping pairs: {}", self.overlapping_pairs());
    }
}

/// Wayland 메시지의 머리. 모든 메시지는 이 8바이트로 시작한다.
///
/// - 첫 4바이트: 이 메시지를 보낸(또는 받을) 객체의 id
/// - 다음 4바이트: 위 16비트는 머리를 포함한 메시지 전체 길이, 아래 16비트는 opcode
///
/// 숫자는 모두 이 기계의 바이트 순서(native endian)로 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    object: u32,
    opcode: u16,
    size: u16,
}

const HEADER_SIZE: usize = 8;

/// 메시지를 읽다가 생길 수 있는 문제들.
#[derive(Debug, PartialEq, Eq)]
enum WireError {
    /// 인자를 읽으려는데 바이트가 모자란다.
    Truncated { needed: usize, available: usize },
    /// 머리에 적힌 길이가 머리보다 짧거나 4의 배수가 아니다.
    BadSize(u16),
    /// 문자열의 길이가 0이거나, 끝에 0 바이트가 없다.
    BadString,
    /// 문자열이 UTF-8이 아니다.
    NotUtf8,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Truncated { needed, available } => {
                write!(f, "needed {needed} bytes but only {available} are left")
            }
            WireError::BadSize(size) => write!(f, "message size {size} is invalid"),
            WireError::BadString => write!(f, "string is not terminated by a 0 byte"),
            WireError::NotUtf8 => write!(f, "string is not valid UTF-8"),
        }
    }
}

impl std::error::Error for WireError {}

/// 4바이트를 숫자 하나로. 길이가 정확히 4인 조각만 받는다.
fn word(bytes: &[u8]) -> [u8; 4] {
    bytes.try_into().expect("caller passes exactly 4 bytes")
}

/// 바이트 앞부분에서 메시지 하나를 잘라 낸다.
///
/// - 메시지 하나가 다 도착했으면 `Ok(Some((머리, 인자 바이트)))`
/// - 아직 덜 왔으면 `Ok(None)` — 더 받은 뒤 다시 부르면 된다
/// - 머리부터 말이 안 되면 `Err`
fn split_message(bytes: &[u8]) -> Result<Option<(Header, &[u8])>, WireError> {
    if bytes.len() < HEADER_SIZE {
        return Ok(None);
    }
    let object = u32::from_ne_bytes(word(&bytes[0..4]));
    let packed = u32::from_ne_bytes(word(&bytes[4..8]));
    let header = Header {
        object,
        opcode: (packed & 0xffff) as u16,
        size: (packed >> 16) as u16,
    };
    let size = usize::from(header.size);
    if size < HEADER_SIZE || !size.is_multiple_of(4) {
        return Err(WireError::BadSize(header.size));
    }
    if bytes.len() < size {
        return Ok(None);
    }
    Ok(Some((header, &bytes[HEADER_SIZE..size])))
}

/// 메시지 하나를 바이트로 조립한다. 인자를 순서대로 붙인 뒤 `finish`로 머리를 씌운다.
struct MessageBuilder {
    object: u32,
    opcode: u16,
    args: Vec<u8>,
}

impl MessageBuilder {
    fn new(object: u32, opcode: u16) -> MessageBuilder {
        MessageBuilder {
            object,
            opcode,
            args: Vec::new(),
        }
    }

    fn uint(mut self, value: u32) -> MessageBuilder {
        self.args.extend_from_slice(&value.to_ne_bytes());
        self
    }

    fn int(mut self, value: i32) -> MessageBuilder {
        self.args.extend_from_slice(&value.to_ne_bytes());
        self
    }

    /// 문자열: 길이(끝의 0 바이트 포함) → 글자 → 0 바이트 → 4의 배수가 되도록 0으로 채움.
    fn string(mut self, value: &str) -> MessageBuilder {
        let length = u32::try_from(value.len() + 1).expect("string length fits in u32");
        self = self.uint(length);
        self.args.extend_from_slice(value.as_bytes());
        self.args.push(0);
        while !self.args.len().is_multiple_of(4) {
            self.args.push(0);
        }
        self
    }

    fn finish(self) -> Vec<u8> {
        let size = HEADER_SIZE + self.args.len();
        let size = u32::try_from(size).expect("message size fits in u32");
        let word = (size << 16) | u32::from(self.opcode);
        let mut bytes = Vec::with_capacity(HEADER_SIZE + self.args.len());
        bytes.extend_from_slice(&self.object.to_ne_bytes());
        bytes.extend_from_slice(&word.to_ne_bytes());
        bytes.extend_from_slice(&self.args);
        bytes
    }
}

/// 메시지의 인자 부분을 앞에서부터 하나씩 읽는다.
///
/// 읽는 동안 원래 바이트를 빌려 들고 있으므로, 그 바이트가 얼마나 살아 있는지를
/// 타입에 `'a`로 적는다.
struct Args<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Args<'a> {
    fn new(bytes: &'a [u8]) -> Args<'a> {
        Args { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], WireError> {
        let available = self.bytes.len() - self.position;
        if count > available {
            return Err(WireError::Truncated {
                needed: count,
                available,
            });
        }
        let chunk = &self.bytes[self.position..self.position + count];
        self.position += count;
        Ok(chunk)
    }

    fn uint(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_ne_bytes(word(self.take(4)?)))
    }

    fn int(&mut self) -> Result<i32, WireError> {
        Ok(i32::from_ne_bytes(word(self.take(4)?)))
    }

    fn string(&mut self) -> Result<&'a str, WireError> {
        let length = usize::try_from(self.uint()?).expect("u32 fits in usize");
        if length == 0 {
            return Err(WireError::BadString);
        }
        let padded = length.div_ceil(4) * 4;
        let chunk = self.take(padded)?;
        let (text, rest) = chunk.split_at(length - 1);
        if rest[0] != 0 {
            return Err(WireError::BadString);
        }
        std::str::from_utf8(text).map_err(|_| WireError::NotUtf8)
    }
}

/// 바이트를 4개씩 끊어 16진수로 보여 준다.
fn hex(bytes: &[u8]) -> String {
    let words: Vec<String> = bytes
        .chunks(4)
        .map(|word| word.iter().map(|b| format!("{b:02x}")).collect())
        .collect();
    words.join(" ")
}

fn wire_demo() -> Result<(), WireError> {
    // 클라이언트가 접속하자마자 보내는 두 요청
    // wl_display(객체 1).get_registry(새 객체 2)  — opcode 1
    // wl_display(객체 1).sync(새 객체 3)          — opcode 0
    let get_registry = MessageBuilder::new(1, 1).uint(2).finish();
    let sync = MessageBuilder::new(1, 0).uint(3).finish();
    println!("get_registry: {}", hex(&get_registry));
    println!("sync:         {}", hex(&sync));

    // 두 메시지가 이어 붙어 왔고, 세 번째 메시지는 앞 5바이트만 도착했다고 치자.
    let mut stream = get_registry.clone();
    stream.extend_from_slice(&sync);
    stream.extend_from_slice(&sync[..5]);
    let mut rest = stream.as_slice();
    while let Some((header, body)) = split_message(rest)? {
        let new_id = Args::new(body).uint()?;
        println!("{header:?} new_id={new_id}");
        rest = &rest[usize::from(header.size)..];
    }
    println!("waiting for {} more byte(s)", HEADER_SIZE - rest.len());

    // 망가진 메시지들
    let too_small = MessageBuilder::new(1, 0).finish();
    let mut bad_size = too_small.clone();
    bad_size[6] = 6; // 길이를 6으로: 머리(8바이트)보다 짧다
    println!("bad size:   {:?}", split_message(&bad_size));

    let short = MessageBuilder::new(1, 0).finish();
    let (_, body) = split_message(&short)?.expect("complete");
    match Args::new(body).uint() {
        Ok(value) => println!("got {value}"),
        Err(error) => println!("short body: {error}"),
    }

    let mut not_utf8 = MessageBuilder::new(2, 0).string("ab").finish();
    not_utf8[12] = 0xff;
    let (_, body) = split_message(&not_utf8)?.expect("complete");
    if let Err(error) = Args::new(body).string() {
        println!("bad string: {error}");
    }

    // 음수 좌표도 같은 4바이트에 담긴다
    let attach = MessageBuilder::new(5, 1).uint(0).int(-20).int(10).finish();
    let (_, body) = split_message(&attach)?.expect("complete");
    let mut args = Args::new(body);
    let (buffer, x, y) = (args.uint()?, args.int()?, args.int()?);
    println!("attach: buffer={buffer} x={x} y={y}");

    // ?는 에러를 만나면 그 자리에서 함수를 끝내고 에러를 호출한 쪽으로 돌려준다.
    let (_, body) = split_message(&short)?.expect("complete");
    let value = Args::new(body).uint()?;
    println!("never printed: {value}");
    Ok(())
}

/// 클라이언트가 만든 객체 하나의 종류.
///
/// 클라이언트는 객체를 만들 때 번호(id)를 정해서 알려 주고, 그 뒤로는 번호로만 부른다.
/// 컴포지터는 번호마다 그것이 무슨 객체인지 적어 둬야 메시지를 알아들을 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Object {
    Display,
    Registry,
    Callback,
    Compositor,
}

impl Object {
    fn interface(self) -> &'static str {
        match self {
            Object::Display => "wl_display",
            Object::Registry => "wl_registry",
            Object::Callback => "wl_callback",
            Object::Compositor => "wl_compositor",
        }
    }
}

/// 컴포지터가 클라이언트에게 "이런 기능이 있다"고 알리는 전역 객체.
struct Global {
    name: u32,
    interface: &'static str,
    version: u32,
}

const GLOBALS: &[Global] = &[Global {
    name: 1,
    interface: "wl_compositor",
    version: 4,
}];

/// 클라이언트가 규칙을 어겼을 때. 클라이언트에게 `wl_display.error`로 알리고 연결을 끊는다.
#[derive(Debug, PartialEq, Eq)]
enum ProtocolError {
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
    fn object_and_code(&self) -> (u32, u32) {
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
struct Client {
    objects: std::collections::HashMap<u32, Object>,
    outgoing: Vec<u8>,
    next_serial: u32,
}

impl Client {
    fn new() -> Client {
        let mut objects = std::collections::HashMap::new();
        // 객체 1번은 언제나 wl_display다. 접속하는 순간부터 있다.
        objects.insert(1, Object::Display);
        Client {
            objects,
            outgoing: Vec::new(),
            next_serial: 1,
        }
    }

    fn send(&mut self, message: Vec<u8>) {
        self.outgoing.extend_from_slice(&message);
    }

    /// 새 객체를 표에 넣는다. 이미 있는 id면 에러.
    fn create(&mut self, id: u32, object: Object) -> Result<(), ProtocolError> {
        use std::collections::hash_map::Entry;
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
    fn receive(&mut self, bytes: &[u8]) -> Result<usize, ProtocolError> {
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
    fn send_error(&mut self, error: &ProtocolError) {
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

/// 컴포지터가 보낸 이벤트를 사람이 읽을 수 있게 한 줄로. (대본용 — 객체 번호를 알고 있다고 친다.)
fn describe_event(header: Header, body: &[u8]) -> Result<String, WireError> {
    let mut args = Args::new(body);
    let text = match (header.object, header.opcode) {
        (1, 0) => format!(
            "wl_display.error(object {}, code {}, {:?})",
            args.uint()?,
            args.uint()?,
            args.string()?
        ),
        (1, 1) => format!("wl_display.delete_id({})", args.uint()?),
        (2, 0) => format!(
            "wl_registry.global({}, {:?}, {})",
            args.uint()?,
            args.string()?,
            args.uint()?
        ),
        (id, 0) => format!("wl_callback#{id}.done({})", args.uint()?),
        (id, opcode) => format!("object {id} event {opcode}"),
    };
    Ok(text)
}

fn print_events(bytes: &[u8]) -> Result<(), WireError> {
    let mut rest = bytes;
    while let Some((header, body)) = split_message(rest)? {
        println!("  <- {}", describe_event(header, body)?);
        rest = &rest[usize::from(header.size)..];
    }
    Ok(())
}

fn protocol_demo() -> Result<(), WireError> {
    let mut client = Client::new();

    // 클라이언트가 보낸다고 치는 요청들: get_registry(2), sync(3), bind(1, wl_compositor, 4, id 4)
    let mut requests = MessageBuilder::new(1, 1).uint(2).finish();
    requests.extend(MessageBuilder::new(1, 0).uint(3).finish());
    requests.extend(
        MessageBuilder::new(2, 0)
            .uint(1)
            .string("wl_compositor")
            .uint(4)
            .uint(4)
            .finish(),
    );
    println!("-> get_registry(2), sync(3), bind(1, \"wl_compositor\", 4, id 4)");
    match client.receive(&requests) {
        Ok(used) => println!("  handled {used} bytes"),
        Err(error) => println!("  error: {error}"),
    }
    print_events(&std::mem::take(&mut client.outgoing))?;

    // HashMap은 순서를 지키지 않으므로, 보여 줄 때는 id 순으로 정렬한다.
    let mut objects: Vec<(u32, &str)> = client
        .objects
        .iter()
        .map(|(id, object)| (*id, object.interface()))
        .collect();
    objects.sort_by_key(|(id, _)| *id);
    println!("objects: {objects:?}");

    // 규칙 위반: 없는 객체 9에 메시지를 보낸다
    println!("-> object 9, opcode 0");
    let bad = MessageBuilder::new(9, 0).finish();
    if let Err(error) = client.receive(&bad) {
        client.send_error(&error);
    }
    print_events(&std::mem::take(&mut client.outgoing))?;

    // 규칙 위반: 이미 있는 id 2로 레지스트리를 또 만든다
    println!("-> get_registry(2) again");
    let again = MessageBuilder::new(1, 1).uint(2).finish();
    if let Err(error) = client.receive(&again) {
        client.send_error(&error);
    }
    print_events(&std::mem::take(&mut client.outgoing))?;
    Ok(())
}

/// 사용자가 입력한 명령 한 줄.
#[derive(Debug, PartialEq)]
enum Command {
    Open(String),
    Close(u32),
    Focus(u32),
    Next,
    Click { x: i32, y: i32 },
    List,
    Help,
    Quit,
}

/// 명령 한 줄을 해석한다. 알아들을 수 없는 줄이면 `None`.
fn parse_command(line: &str) -> Option<Command> {
    let mut words = line.split_whitespace();
    let name = words.next()?;
    let command = match name {
        "open" => {
            let title: Vec<&str> = words.collect();
            if title.is_empty() {
                return None;
            }
            Command::Open(title.join(" "))
        }
        "close" => Command::Close(words.next()?.parse().ok()?),
        "focus" => Command::Focus(words.next()?.parse().ok()?),
        "next" => Command::Next,
        "click" => {
            let x = words.next()?.parse().ok()?;
            let y = words.next()?.parse().ok()?;
            Command::Click { x, y }
        }
        "list" => Command::List,
        "help" => Command::Help,
        "quit" | "exit" => Command::Quit,
        _ => return None,
    };
    Some(command)
}

const HELP: &str =
    "commands: open <title> | close <id> | focus <id> | next | click <x> <y> | list | help | quit";

fn main() {
    // `cargo run -- wire`로 실행하면 메시지 형식을 보여 주고 끝난다.
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "wire" {
        if let Err(error) = wire_demo() {
            println!("wire demo stopped: {error}");
        }
        return;
    }
    if mode == "protocol" {
        if let Err(error) = protocol_demo() {
            println!("protocol demo stopped: {error}");
        }
        return;
    }

    let mut desktop = Desktop::new(Rect::new(0, 0, 1920, 1080));
    println!("{HELP}");

    for line in std::io::stdin().lines() {
        let line = line.expect("stdin is readable");
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(command) = parse_command(line) else {
            println!("? {line:?} (type help)");
            continue;
        };
        match command {
            Command::Open(title) => {
                let id = desktop.open(&title);
                println!("opened #{id}");
            }
            Command::Close(id) => {
                if !desktop.close(id) {
                    println!("no window #{id}");
                }
            }
            Command::Focus(id) => {
                if !desktop.focus(id) {
                    println!("no window #{id}");
                }
            }
            Command::Next => desktop.focus_next(),
            Command::Click { x, y } => match desktop.window_at(x, y) {
                Some(window) => {
                    let id = window.id;
                    desktop.focus(id);
                    println!("focused #{id}");
                }
                None => println!("nothing at ({x}, {y})"),
            },
            Command::List => desktop.print(),
            Command::Help => println!("{HELP}"),
            Command::Quit => break,
        }
        if let Some(window) = desktop.focused_window() {
            println!("[focus: #{} {}]", window.id, window.title);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_cover_total_without_gaps() {
        for total in [0, 1, 7, 1000, 1920] {
            for count in 1..=7 {
                let mut next_start = 0;
                for index in 0..count {
                    let (start, length) = span(total, count, index);
                    assert_eq!(
                        start, next_start,
                        "total={total} count={count} index={index}"
                    );
                    next_start = start + length;
                }
                assert_eq!(next_start, total, "total={total} count={count}");
            }
        }
    }

    #[test]
    fn tiles_do_not_overlap() {
        let area = Rect::new(-100, 20, 1000, 500);
        let rects = tile(area, 3);
        assert_eq!(rects.len(), 3);
        assert_eq!(rects[0], Rect::new(-100, 20, 334, 500));
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                assert!(
                    !rects[i].intersects(&rects[j]),
                    "{:?} and {:?}",
                    rects[i],
                    rects[j]
                );
            }
        }
    }

    #[test]
    fn rect_edges_are_exclusive() {
        let r = Rect::new(0, 0, 10, 10);
        assert!(r.contains(0, 0));
        assert!(r.contains(9, 9));
        assert!(!r.contains(10, 0));
        assert!(!r.intersects(&Rect::new(10, 0, 5, 5)));
    }

    #[test]
    fn new_window_takes_focus() {
        let mut desktop = Desktop::new(Rect::new(0, 0, 100, 100));
        assert_eq!(desktop.focused, None);
        let a = desktop.open("a");
        let b = desktop.open("b");
        assert_eq!(desktop.focused, Some(b));
        assert!(desktop.focus(a));
        assert!(!desktop.focus(99));
        assert_eq!(desktop.focused, Some(a));
    }

    #[test]
    fn closing_focused_window_moves_focus() {
        let mut desktop = Desktop::new(Rect::new(0, 0, 100, 100));
        let a = desktop.open("a");
        let b = desktop.open("b");
        let c = desktop.open("c");
        desktop.focus(b);
        assert!(desktop.close(b));
        assert_eq!(desktop.focused, Some(c), "the window that took b's place");
        assert!(desktop.close(c));
        assert_eq!(desktop.focused, Some(a), "the last window");
        assert!(desktop.close(a));
        assert_eq!(desktop.focused, None);
        assert!(!desktop.close(a));
    }

    #[test]
    fn focus_next_wraps_around() {
        let mut desktop = Desktop::new(Rect::new(0, 0, 100, 100));
        let a = desktop.open("a");
        let b = desktop.open("b");
        desktop.focus_next();
        assert_eq!(desktop.focused, Some(a));
        desktop.focus_next();
        assert_eq!(desktop.focused, Some(b));
    }

    #[test]
    fn header_round_trip() {
        let bytes = MessageBuilder::new(7, 3).uint(42).finish();
        assert_eq!(bytes.len(), 12);
        let (header, body) = split_message(&bytes).unwrap().unwrap();
        assert_eq!(
            header,
            Header {
                object: 7,
                opcode: 3,
                size: 12
            }
        );
        assert_eq!(Args::new(body).uint(), Ok(42));
    }

    #[test]
    fn incomplete_messages_wait_for_more() {
        let bytes = MessageBuilder::new(7, 3).uint(42).finish();
        for cut in 0..bytes.len() {
            assert_eq!(split_message(&bytes[..cut]), Ok(None), "cut at {cut}");
        }
    }

    #[test]
    fn rejects_bad_sizes() {
        let mut bytes = MessageBuilder::new(1, 0).uint(0).finish();
        bytes[6] = 10; // 4의 배수가 아니다
        assert_eq!(split_message(&bytes), Err(WireError::BadSize(10)));
    }

    #[test]
    fn strings_are_padded_to_four_bytes() {
        for text in ["", "a", "abc", "abcd", "wl_compositor"] {
            let bytes = MessageBuilder::new(1, 0).string(text).uint(99).finish();
            assert_eq!(bytes.len() % 4, 0);
            let (_, body) = split_message(&bytes).unwrap().unwrap();
            let mut args = Args::new(body);
            assert_eq!(args.string(), Ok(text));
            assert_eq!(args.uint(), Ok(99));
        }
    }

    #[test]
    fn reading_past_the_end_fails() {
        let bytes = MessageBuilder::new(1, 0).uint(1).finish();
        let mut args = Args::new(&bytes[HEADER_SIZE..]);
        assert_eq!(args.uint(), Ok(1));
        assert_eq!(
            args.uint(),
            Err(WireError::Truncated {
                needed: 4,
                available: 0
            })
        );
    }

    #[test]
    fn rejects_bad_strings() {
        let mut bytes = MessageBuilder::new(1, 0).string("abc").finish();
        bytes[HEADER_SIZE + 4 + 3] = b'!'; // 끝의 0 바이트를 덮어쓴다
        assert_eq!(
            Args::new(&bytes[HEADER_SIZE..]).string(),
            Err(WireError::BadString)
        );
        let zero_length = 0u32.to_ne_bytes();
        assert_eq!(Args::new(&zero_length).string(), Err(WireError::BadString));
    }

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

    #[test]
    fn parses_commands() {
        assert_eq!(
            parse_command("open web browser"),
            Some(Command::Open("web browser".to_string()))
        );
        assert_eq!(parse_command("close 3"), Some(Command::Close(3)));
        assert_eq!(
            parse_command("click -5 10"),
            Some(Command::Click { x: -5, y: 10 })
        );
        assert_eq!(parse_command("exit"), Some(Command::Quit));
        assert_eq!(parse_command("open"), None);
        assert_eq!(parse_command("close x"), None);
        assert_eq!(parse_command("click 1"), None);
        assert_eq!(parse_command("fly"), None);
    }
}
