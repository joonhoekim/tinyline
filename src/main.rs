fn main() {
    // 컴포지터가 그릴 화면(출력) 하나의 크기
    let width: u32 = 1920;
    let height: u32 = 1080;

    let pixels = width * height;
    println!("output: {width}x{height} ({pixels} pixels)");

    // 한 픽셀은 4바이트(파랑·초록·빨강·투명도)
    let bytes_per_pixel = 4;
    let frame_bytes = pixels * bytes_per_pixel;
    println!("one frame: {frame_bytes} bytes");

    let mut windows = 0;
    windows += 1;
    windows += 1;
    println!("windows open: {windows}");
}
