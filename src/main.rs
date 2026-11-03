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

fn main() {
    let output = Rect::new(0, 0, 1920, 1080);
    println!("output: {output:?}");
    println!("output area: {} pixels", output.area());

    let terminal = Rect::new(100, 100, 800, 600);
    let browser = Rect::new(600, 400, 1000, 700);
    let offscreen = Rect::new(-500, 50, 300, 200);

    println!(
        "terminal and browser overlap: {}",
        terminal.intersects(&browser)
    );
    println!("offscreen on output: {}", offscreen.intersects(&output));
    println!(
        "click (650, 450) hits terminal: {}",
        terminal.contains(650, 450)
    );
    println!(
        "click (650, 450) hits browser: {}",
        browser.contains(650, 450)
    );

    let copy = terminal;
    println!("copy == terminal: {}", copy == terminal);
}
