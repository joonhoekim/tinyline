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
}

impl Desktop {
    fn new(output: Rect) -> Desktop {
        Desktop {
            output,
            windows: Vec::new(),
            next_id: 1,
        }
    }

    /// 새 창을 열고 그 id를 돌려준다. 창을 열 때마다 화면을 다시 나눈다.
    fn open(&mut self, title: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.windows.push(Window {
            id,
            title: title.to_string(),
            rect: self.output,
        });
        self.relayout();
        id
    }

    /// id에 해당하는 창을 닫는다. 그런 창이 없었으면 false.
    fn close(&mut self, id: u32) -> bool {
        let before = self.windows.len();
        self.windows.retain(|w| w.id != id);
        let closed = self.windows.len() != before;
        if closed {
            self.relayout();
        }
        closed
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
    println!("overlapping pairs: {}", desktop.overlapping_pairs());

    // 나누어떨어지지 않을 때: 1000을 세 칸으로
    let widths: Vec<u32> = tile(Rect::new(0, 0, 1000, 100), 3)
        .iter()
        .map(|r| r.width)
        .collect();
    println!("1000 into 3 columns: {widths:?}");
}
