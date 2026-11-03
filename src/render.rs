//! 여러 창의 그림을 출력 한 장으로 합치고, 이미지 파일로 저장한다.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::layout::Rect;
use crate::protocol::Image;

/// 화면에 나갈 그림 한 장. 픽셀 하나는 0xAARRGGBB.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

/// 0xAARRGGBB에서 채널 하나(0=파랑, 1=초록, 2=빨강, 3=투명도)를 꺼낸다.
fn channel(pixel: u32, index: u32) -> u32 {
    (pixel >> (index * 8)) & 0xff
}

/// `src`를 `dst` 위에 얹는다.
///
/// Wayland의 argb8888은 색에 투명도가 이미 곱해져 있다(premultiplied alpha).
/// 그래서 얹는 식이 간단하다: 결과 = 위 + 아래 × (1 − 위의 투명도)
pub fn blend(src: u32, dst: u32) -> u32 {
    let alpha = channel(src, 3);
    if alpha == 0xff {
        return src;
    }
    let mut out = 0;
    for index in 0..4 {
        let value = channel(src, index) + channel(dst, index) * (255 - alpha) / 255;
        // 클라이언트가 규칙을 어겨 색이 투명도보다 크면 255를 넘을 수 있다. 잘라 낸다.
        out |= value.min(255) << (index * 8);
    }
    out
}

impl Frame {
    pub fn new(width: u32, height: u32, background: u32) -> Frame {
        Frame {
            width,
            height,
            pixels: vec![background; width as usize * height as usize],
        }
    }

    /// 화면 밖이면 아무 일도 하지 않는다.
    fn put(&mut self, x: i32, y: i32, pixel: u32) {
        let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
            return;
        };
        if x >= self.width || y >= self.height {
            return;
        }
        let index = y as usize * self.width as usize + x as usize;
        self.pixels[index] = blend(pixel, self.pixels[index]);
    }

    /// 그림을 `area`의 왼쪽 위에 맞춰 그린다. `area`를 넘치는 부분은 잘린다.
    pub fn draw(&mut self, image: &Image, area: Rect) {
        let width = image.width.min(area.width);
        let height = image.height.min(area.height);
        for y in 0..height {
            for x in 0..width {
                let pixel = image.pixels[y as usize * image.width as usize + x as usize];
                self.put(area.x + x as i32, area.y + y as i32, pixel);
            }
        }
    }

    /// `area` 안쪽 둘레에 두께 `thickness`의 테두리를 그린다.
    pub fn outline(&mut self, area: Rect, thickness: u32, color: u32) {
        for y in 0..area.height {
            for x in 0..area.width {
                let edge = x < thickness
                    || y < thickness
                    || x + thickness >= area.width
                    || y + thickness >= area.height;
                if edge {
                    self.put(area.x + x as i32, area.y + y as i32, color);
                }
            }
        }
    }

    /// PPM(P6) 형식으로 저장한다: 짧은 글자 머리 + 픽셀마다 빨강·초록·파랑 1바이트씩.
    pub fn save_ppm(&self, path: &Path) -> io::Result<()> {
        let mut out = BufWriter::new(File::create(path)?);
        write!(out, "P6\n{} {}\n255\n", self.width, self.height)?;
        for pixel in &self.pixels {
            out.write_all(&[
                channel(*pixel, 2) as u8,
                channel(*pixel, 1) as u8,
                channel(*pixel, 0) as u8,
            ])?;
        }
        out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_pixels_replace() {
        assert_eq!(blend(0xff112233, 0xffffffff), 0xff112233);
    }

    #[test]
    fn transparent_pixels_keep_background() {
        assert_eq!(blend(0x00000000, 0xff445566), 0xff445566);
    }

    #[test]
    fn half_transparent_red_over_blue() {
        // 투명도 0x80인 빨강(premultiplied이므로 빨강도 0x80) 위에 파랑 배경
        assert_eq!(blend(0x80800000, 0xff0000ff), 0xff80007f);
    }

    #[test]
    fn drawing_is_clipped_to_the_area_and_frame() {
        let mut frame = Frame::new(4, 4, 0xff000000);
        let image = Image {
            width: 3,
            height: 3,
            pixels: vec![0xffffffff; 9],
        };
        frame.draw(&image, Rect::new(2, 2, 2, 2));
        let white = frame.pixels.iter().filter(|p| **p == 0xffffffff).count();
        assert_eq!(white, 4);
        frame.draw(&image, Rect::new(-2, -2, 3, 3));
        assert_eq!(frame.pixels[0], 0xffffffff);
    }
}
