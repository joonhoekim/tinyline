//! 화면과 창: 사각형, 배치, 포커스.

/// 화면 위의 사각형 하나. 왼쪽 위 모서리(x, y)와 너비·높이로 정한다.
///
/// 위치는 음수가 될 수 있다(창을 화면 왼쪽 밖으로 끌어낼 수 있다).
/// 크기는 음수가 될 수 없으므로 부호 없는 정수로 둔다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// 오른쪽 끝의 x 좌표(이 값 자체는 사각형에 들어가지 않는다).
    pub fn right(&self) -> i32 {
        self.x + i32::try_from(self.width).expect("width fits in i32")
    }

    /// 아래쪽 끝의 y 좌표(이 값 자체는 사각형에 들어가지 않는다).
    pub fn bottom(&self) -> i32 {
        self.y + i32::try_from(self.height).expect("height fits in i32")
    }

    pub fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

/// 컴포지터가 관리하는 창 하나.
#[derive(Debug)]
pub struct Window {
    pub id: u32,
    pub title: String,
    pub rect: Rect,
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
pub fn tile(area: Rect, count: usize) -> Vec<Rect> {
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
pub struct Desktop {
    output: Rect,
    windows: Vec<Window>,
    next_id: u32,
    /// 키보드 입력을 받을 창. 창이 하나도 없으면 없다.
    focused: Option<u32>,
}

impl Desktop {
    pub fn new(output: Rect) -> Desktop {
        Desktop {
            output,
            windows: Vec::new(),
            next_id: 1,
            focused: None,
        }
    }

    /// 창들이 놓이는 화면.
    pub fn output(&self) -> Rect {
        self.output
    }

    /// 열린 창들. 고치려면 `open`·`close`·`focus`를 거쳐야 한다.
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    pub fn focused(&self) -> Option<u32> {
        self.focused
    }

    /// 새 창을 열고 그 id를 돌려준다. 창을 열 때마다 화면을 다시 나누고,
    /// 새 창에 포커스를 준다.
    pub fn open(&mut self, title: &str) -> u32 {
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
    pub fn close(&mut self, id: u32) -> bool {
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

    /// 창 제목을 바꾼다. 그런 창이 없으면 false.
    pub fn set_title(&mut self, id: u32, title: &str) -> bool {
        match self.windows.iter_mut().find(|w| w.id == id) {
            Some(window) => {
                window.title = title.to_string();
                true
            }
            None => false,
        }
    }

    /// id에 해당하는 창에 포커스를 준다. 그런 창이 없으면 false.
    pub fn focus(&mut self, id: u32) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        self.focused = Some(id);
        true
    }

    /// 포커스를 오른쪽 창으로 옮긴다. 마지막 창에서는 첫 창으로 돌아간다.
    pub fn focus_next(&mut self) {
        if self.windows.is_empty() {
            return;
        }
        let next = match self.focused.and_then(|id| self.index_of(id)) {
            Some(index) => (index + 1) % self.windows.len(),
            None => 0,
        };
        self.focused = Some(self.windows[next].id);
    }

    pub fn focused_window(&self) -> Option<&Window> {
        let id = self.focused?;
        self.windows.iter().find(|w| w.id == id)
    }

    /// (x, y)를 클릭하면 맞는 창. 나중에 연 창이 위에 있으므로 뒤에서부터 찾는다.
    pub fn window_at(&self, x: i32, y: i32) -> Option<&Window> {
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

    pub fn print(&self) {
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
}
