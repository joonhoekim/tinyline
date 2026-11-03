//! 시험용 Wayland 클라이언트. 컴포지터에 붙어 공유 메모리에 그림을 그려 넘긴다.
//!
//! ```sh
//! cargo run -- serve                                          # 터미널 1
//! WAYLAND_DISPLAY=tinyline-0 cargo run --example paint        # 터미널 2
//! WAYLAND_DISPLAY=tinyline-0 cargo run --example paint -- 5 2e8b57   # 5초 동안, 초록으로
//! ```

use std::io::{self, IoSlice, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;

use rustix::fs::{MemfdFlags, memfd_create};
use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
use tinyline::wire::{Args, MessageBuilder, WireError, split_message};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

/// 컴포지터와의 연결. 이벤트를 받아 그대로 쌓아 둔다.
struct Connection {
    stream: UnixStream,
    pending: Vec<u8>,
    next_id: u32,
}

impl Connection {
    fn connect() -> io::Result<Connection> {
        let dir = std::env::var("XDG_RUNTIME_DIR")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set"))?;
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "tinyline-0".into());
        let stream = UnixStream::connect(format!("{dir}/{display}"))?;
        // 1번은 wl_display. 클라이언트가 만드는 객체는 2번부터.
        Ok(Connection {
            stream,
            pending: Vec::new(),
            next_id: 2,
        })
    }

    fn new_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn send(&mut self, message: Vec<u8>) -> io::Result<()> {
        self.stream.write_all(&message)
    }

    /// 메시지 하나를 fd 하나와 함께 보낸다.
    fn send_with_fd(&mut self, message: Vec<u8>, fd: impl AsFd) -> io::Result<()> {
        let fds = [fd.as_fd()];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        control.push(SendAncillaryMessage::ScmRights(&fds));
        let sent = sendmsg(
            &self.stream,
            &[IoSlice::new(&message)],
            &mut control,
            SendFlags::empty(),
        )?;
        assert_eq!(sent, message.len(), "short send");
        Ok(())
    }

    /// `wl_display.sync`를 보내고, 그 답이 올 때까지 이벤트를 받아 `on_event`에 넘긴다.
    /// sync의 답은 앞서 보낸 요청이 모두 처리된 뒤에 오므로, 왕복 한 번을 기다리는 셈이다.
    fn roundtrip(&mut self, mut on_event: impl FnMut(u32, u16, Args<'_>)) -> io::Result<()> {
        let callback = self.new_id();
        self.send(MessageBuilder::new(1, 0).uint(callback).finish())?;
        let mut chunk = [0u8; 4096];
        loop {
            let count = self.stream.read(&mut chunk)?;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "compositor hung up",
                ));
            }
            self.pending.extend_from_slice(&chunk[..count]);
            let mut used = 0;
            let mut done = false;
            while let Some((header, body)) = split_message(&self.pending[used..]).map_err(wire)? {
                used += usize::from(header.size);
                if header.object == 1 && header.opcode == 0 {
                    let mut args = Args::new(body);
                    let (object, code) = (args.uint().map_err(wire)?, args.uint().map_err(wire)?);
                    let message = args.string().map_err(wire)?;
                    return Err(io::Error::other(format!(
                        "protocol error on object {object} (code {code}): {message}"
                    )));
                }
                if header.object == callback {
                    done = true;
                } else {
                    on_event(header.object, header.opcode, Args::new(body));
                }
            }
            self.pending.drain(..used);
            if done {
                return Ok(());
            }
        }
    }
}

fn wire(error: WireError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

/// 바둑판 무늬. 픽셀 하나는 0xAARRGGBB(작은 쪽 바이트부터 메모리에 놓인다).
fn checkerboard(x: u32, y: u32, color: u32) -> u32 {
    if (x / 16 + y / 16).is_multiple_of(2) {
        0xff00_0000 | color
    } else {
        0xff_20_20_40 // 짙은 남색
    }
}

fn main() -> io::Result<()> {
    // 인자: [머무를 초] [색 RRGGBB]
    let mut args = std::env::args().skip(1);
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let color = args
        .next()
        .and_then(|s| u32::from_str_radix(&s, 16).ok())
        .unwrap_or(0xff_88_00);

    let mut conn = Connection::connect()?;

    // 1. 어떤 전역 객체가 있는지 묻는다.
    let registry = conn.new_id();
    conn.send(MessageBuilder::new(1, 1).uint(registry).finish())?;
    let mut globals: Vec<(u32, String, u32)> = Vec::new();
    conn.roundtrip(|object, opcode, mut args| {
        if object == registry && opcode == 0 {
            let name = args.uint().expect("name");
            let interface = args.string().expect("interface").to_string();
            let version = args.uint().expect("version");
            globals.push((name, interface, version));
        }
    })?;
    println!("globals: {globals:?}");

    // 2. wl_shm에 붙는다.
    let (shm_name, _, _) = globals
        .iter()
        .find(|(_, interface, _)| interface == "wl_shm")
        .expect("compositor has wl_shm");
    let shm = conn.new_id();
    conn.send(
        MessageBuilder::new(registry, 0)
            .uint(*shm_name)
            .string("wl_shm")
            .uint(1)
            .uint(shm)
            .finish(),
    )?;
    let mut formats = Vec::new();
    conn.roundtrip(|object, opcode, mut args| {
        if object == shm && opcode == 0 {
            formats.push(args.uint().expect("format"));
        }
    })?;
    println!("wl_shm formats: {formats:?}");

    // 3. 메모리 파일을 만들어 그림을 그린다.
    let stride = WIDTH * 4;
    let size = stride * HEIGHT;
    let memfd = memfd_create("paint", MemfdFlags::CLOEXEC)?;
    let mut file = std::fs::File::from(memfd);
    let mut pixels = Vec::with_capacity(size as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            pixels.extend_from_slice(&checkerboard(x, y, color).to_le_bytes());
        }
    }
    file.write_all(&pixels)?;

    // 4. 그 파일의 fd를 소켓으로 넘겨 풀을 만들고, 풀 안에 버퍼를 하나 만든다.
    let pool = conn.new_id();
    let create_pool = MessageBuilder::new(shm, 0)
        .uint(pool)
        .int(size as i32)
        .finish();
    conn.send_with_fd(create_pool, &file)?;
    let buffer = conn.new_id();
    conn.send(
        MessageBuilder::new(pool, 0)
            .uint(buffer)
            .int(0)
            .int(WIDTH as i32)
            .int(HEIGHT as i32)
            .int(stride as i32)
            .uint(1) // xrgb8888
            .finish(),
    )?;
    println!("buffer#{buffer}: {WIDTH}x{HEIGHT} in pool#{pool}");

    // 5. 표면을 만들고, 버퍼를 붙이고, 다 됐으면 알려 달라(frame)고 한 뒤 commit한다.
    let (compositor_name, _, _) = globals
        .iter()
        .find(|(_, interface, _)| interface == "wl_compositor")
        .expect("compositor has wl_compositor");
    let compositor = conn.new_id();
    conn.send(
        MessageBuilder::new(registry, 0)
            .uint(*compositor_name)
            .string("wl_compositor")
            .uint(4)
            .uint(compositor)
            .finish(),
    )?;
    let surface = conn.new_id();
    conn.send(MessageBuilder::new(compositor, 0).uint(surface).finish())?;
    conn.send(
        MessageBuilder::new(surface, 1)
            .uint(buffer)
            .int(0)
            .int(0)
            .finish(),
    )?;
    conn.send(
        MessageBuilder::new(surface, 2)
            .int(0)
            .int(0)
            .int(WIDTH as i32)
            .int(HEIGHT as i32)
            .finish(),
    )?;
    let frame = conn.new_id();
    conn.send(MessageBuilder::new(surface, 3).uint(frame).finish())?;
    conn.send(MessageBuilder::new(surface, 6).finish())?;
    conn.roundtrip(|object, opcode, mut args| {
        if object == buffer && opcode == 0 {
            println!("buffer#{buffer} released: we may draw into it again");
        } else if object == frame && opcode == 0 {
            println!("frame#{frame} done at {} ms", args.uint().expect("time"));
        }
    })?;
    // 창이 화면에 남아 있게 잠시 연결을 붙들고 있는다.
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    Ok(())
}
