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

/// 화면 하나와 그 위의 창들.
struct Desktop {
    output: Rect,
    windows: Vec<Window>,
    next_id: u32,
}

impl Desktop {
    fn new(output: Rect) -> Desktop {
        Desktop {
            output,
            windows: Vec::new(),
            next_id: 1,
        }
    }

    /// 새 창을 열고 그 id를 돌려준다. 새 창은 화면 전체를 차지한다.
    fn open(&mut self, title: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.windows.push(Window {
            id,
            title: title.to_string(),
            rect: self.output,
        });
        id
    }

    /// id에 해당하는 창을 닫는다. 그런 창이 없었으면 false.
    fn close(&mut self, id: u32) -> bool {
        let before = self.windows.len();
        self.windows.retain(|w| w.id != id);
        self.windows.len() != before
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
            println!(
                "  #{} {:?} at {:?} ({} px)",
                window.id,
                window.title,
                window.rect,
                window.rect.area()
            );
        }
    }
}

fn main() {
    let mut desktop = Desktop::new(Rect::new(0, 0, 1920, 1080));

    let terminal = desktop.open("terminal");
    desktop.open("browser");
    desktop.open("editor");
    desktop.print();
    println!("overlapping pairs: {}", desktop.overlapping_pairs());

    // 클릭한 곳에 있는 창 찾기: 나중에 연 창이 위에 있으므로 뒤에서부터 본다.
    for window in desktop.windows.iter().rev() {
        if window.rect.contains(10, 10) {
            println!("click (10, 10) hits #{} {:?}", window.id, window.title);
            break;
        }
    }

    println!("close #{terminal}: {}", desktop.close(terminal));
    println!("close #{terminal} again: {}", desktop.close(terminal));
    desktop.print();
}
