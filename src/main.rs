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

/// 바이트 앞부분에서 머리를 읽는다. 8바이트가 안 되면 `None`.
fn read_header(bytes: &[u8]) -> Option<Header> {
    let object = u32::from_ne_bytes(bytes.get(0..4)?.try_into().ok()?);
    let word = u32::from_ne_bytes(bytes.get(4..8)?.try_into().ok()?);
    Some(Header {
        object,
        opcode: (word & 0xffff) as u16,
        size: (word >> 16) as u16,
    })
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

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(count)?;
        let chunk = self.bytes.get(self.position..end)?;
        self.position = end;
        Some(chunk)
    }

    fn uint(&mut self) -> Option<u32> {
        Some(u32::from_ne_bytes(self.take(4)?.try_into().ok()?))
    }

    fn int(&mut self) -> Option<i32> {
        Some(i32::from_ne_bytes(self.take(4)?.try_into().ok()?))
    }

    fn string(&mut self) -> Option<&'a str> {
        let length = usize::try_from(self.uint()?).ok()?;
        let padded = length.checked_add(3)? / 4 * 4;
        let chunk = self.take(padded)?;
        let (text, terminator) = chunk.get(..length)?.split_at(length.checked_sub(1)?);
        if terminator != [0] {
            return None;
        }
        std::str::from_utf8(text).ok()
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

fn wire_demo() {
    // 클라이언트가 접속하자마자 보내는 두 요청
    // wl_display(객체 1).get_registry(새 객체 2)  — opcode 1
    // wl_display(객체 1).sync(새 객체 3)          — opcode 0
    let get_registry = MessageBuilder::new(1, 1).uint(2).finish();
    let sync = MessageBuilder::new(1, 0).uint(3).finish();
    println!("get_registry: {}", hex(&get_registry));
    println!("sync:         {}", hex(&sync));

    // 두 메시지가 이어 붙어 소켓으로 온다고 치고, 앞에서부터 잘라 읽는다.
    let mut stream = get_registry.clone();
    stream.extend_from_slice(&sync);
    let mut rest = stream.as_slice();
    while let Some(header) = read_header(rest) {
        let size = usize::from(header.size);
        let mut args = Args::new(&rest[HEADER_SIZE..size]);
        println!("{header:?} new_id={:?}", args.uint());
        rest = &rest[size..];
    }

    // 컴포지터가 보내는 이벤트: wl_registry(객체 2).global(이름 1, "wl_compositor", 버전 6)
    let global = MessageBuilder::new(2, 0)
        .uint(1)
        .string("wl_compositor")
        .uint(6)
        .finish();
    println!("global:       {}", hex(&global));
    let header = read_header(&global).expect("we just built it");
    let mut args = Args::new(&global[HEADER_SIZE..]);
    println!(
        "{header:?} name={:?} interface={:?} version={:?}",
        args.uint(),
        args.string(),
        args.uint()
    );

    // 음수 좌표도 같은 4바이트에 담긴다
    let attach = MessageBuilder::new(5, 1).uint(0).int(-20).int(10).finish();
    let mut args = Args::new(&attach[HEADER_SIZE..]);
    println!(
        "attach args: buffer={:?} x={:?} y={:?}",
        args.uint(),
        args.int(),
        args.int()
    );
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
        wire_demo();
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
        assert_eq!(
            read_header(&bytes),
            Some(Header {
                object: 7,
                opcode: 3,
                size: 12
            })
        );
        assert_eq!(read_header(&bytes[..7]), None);
    }

    #[test]
    fn strings_are_padded_to_four_bytes() {
        for text in ["", "a", "abc", "abcd", "wl_compositor"] {
            let bytes = MessageBuilder::new(1, 0).string(text).uint(99).finish();
            assert_eq!(bytes.len() % 4, 0);
            let mut args = Args::new(&bytes[HEADER_SIZE..]);
            assert_eq!(args.string(), Some(text));
            assert_eq!(args.uint(), Some(99));
        }
    }

    #[test]
    fn reading_past_the_end_fails() {
        let bytes = MessageBuilder::new(1, 0).uint(1).finish();
        let mut args = Args::new(&bytes[HEADER_SIZE..]);
        assert_eq!(args.uint(), Some(1));
        assert_eq!(args.uint(), None);
        assert_eq!(args.int(), None);
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
