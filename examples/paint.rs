//! 시험용 Wayland 클라이언트. 컴포지터에 붙어 공유 메모리에 그림을 그려 넘긴다.
//!
//! ```sh
//! cargo run -- serve                                          # 터미널 1
//! WAYLAND_DISPLAY=tinyline-0 cargo run --example paint        # 터미널 2
//! WAYLAND_DISPLAY=tinyline-0 cargo run --example paint -- 5 2e8b57   # 5초 동안, 초록으로
//!
//! 지금 쓰는 데스크톱의 진짜 컴포지터에도 그대로 붙는다(`WAYLAND_DISPLAY`를 바꾸지 않으면).
//! ```

use std::io::{self, IoSlice, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use rustix::fs::{MemfdFlags, memfd_create};
use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
use tinyline::wire::{Args, MessageBuilder, WireError, split_message};

/// 컴포지터가 크기를 정해 주지 않을 때(0×0) 쓰는 크기.
const DEFAULT_WIDTH: u32 = 320;
const DEFAULT_HEIGHT: u32 = 240;

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

    /// 소켓에서 한 번 읽어, 그 안의 완성된 이벤트를 모두 `on_event`에 넘긴다.
    /// `timeout` 안에 아무것도 오지 않으면 `Ok(false)`.
    fn read_events(
        &mut self,
        timeout: Option<Duration>,
        on_event: &mut impl FnMut(u32, u16, Args<'_>),
    ) -> io::Result<bool> {
        self.stream.set_read_timeout(timeout)?;
        let mut chunk = [0u8; 4096];
        let count = match self.stream.read(&mut chunk) {
            Ok(count) => count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "compositor hung up",
            ));
        }
        self.pending.extend_from_slice(&chunk[..count]);
        let mut used = 0;
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
            on_event(header.object, header.opcode, Args::new(body));
        }
        self.pending.drain(..used);
        Ok(true)
    }

    /// `wl_display.sync`를 보내고, 그 답이 올 때까지 이벤트를 받아 `on_event`에 넘긴다.
    /// sync의 답은 앞서 보낸 요청이 모두 처리된 뒤에 오므로, 왕복 한 번을 기다리는 셈이다.
    fn roundtrip(&mut self, mut on_event: impl FnMut(u32, u16, Args<'_>)) -> io::Result<()> {
        let callback = self.new_id();
        self.send(MessageBuilder::new(1, 0).uint(callback).finish())?;
        let mut done = false;
        while !done {
            self.read_events(None, &mut |object, opcode, args| {
                if object == callback {
                    done = true;
                } else {
                    on_event(object, opcode, args);
                }
            })?;
        }
        Ok(())
    }

    /// 레지스트리에서 `interface`를 찾아 bind하고, 새 객체의 id를 돌려준다.
    fn bind(
        &mut self,
        registry: u32,
        globals: &[(u32, String, u32)],
        interface: &str,
        version: u32,
    ) -> io::Result<u32> {
        let (name, _, _) = globals
            .iter()
            .find(|(_, name, _)| name == interface)
            .ok_or_else(|| io::Error::other(format!("compositor has no {interface}")))?;
        let id = self.new_id();
        self.send(
            MessageBuilder::new(registry, 0)
                .uint(*name)
                .string(interface)
                .uint(version)
                .uint(id)
                .finish(),
        )?;
        Ok(id)
    }

    /// 새 메모리 파일에 바둑판을 그려 버퍼 하나로 만든다.
    fn draw(&mut self, shm: u32, width: u32, height: u32, color: u32) -> io::Result<u32> {
        let stride = width * 4;
        let size = stride * height;
        let memfd = memfd_create("paint", MemfdFlags::CLOEXEC)?;
        let mut file = std::fs::File::from(memfd);
        let mut pixels = Vec::with_capacity(size as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&checkerboard(x, y, color).to_le_bytes());
            }
        }
        file.write_all(&pixels)?;

        let pool = self.new_id();
        let create_pool = MessageBuilder::new(shm, 0)
            .uint(pool)
            .int(size as i32)
            .finish();
        self.send_with_fd(create_pool, &file)?;
        let buffer = self.new_id();
        self.send(
            MessageBuilder::new(pool, 0)
                .uint(buffer)
                .int(0)
                .int(width as i32)
                .int(height as i32)
                .int(stride as i32)
                .uint(1) // xrgb8888
                .finish(),
        )?;
        // 풀은 버리고 버퍼만 남긴다. 버퍼가 있는 동안 메모리는 살아 있다.
        self.send(MessageBuilder::new(pool, 1).finish())?;
        Ok(buffer)
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

    // 1. 어떤 전역 객체가 있는지 묻고, 필요한 셋에 붙는다.
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
    let compositor = conn.bind(registry, &globals, "wl_compositor", 4)?;
    let shm = conn.bind(registry, &globals, "wl_shm", 1)?;
    let wm_base = conn.bind(registry, &globals, "xdg_wm_base", 1)?;

    // 2. 표면을 만들어 창(toplevel) 역할을 준다. 처음 commit은 그림 없이 한다:
    //    "이런 창을 만들려 한다, 크기를 알려 달라".
    let surface = conn.new_id();
    conn.send(MessageBuilder::new(compositor, 0).uint(surface).finish())?;
    let xdg_surface = conn.new_id();
    conn.send(
        MessageBuilder::new(wm_base, 2)
            .uint(xdg_surface)
            .uint(surface)
            .finish(),
    )?;
    let toplevel = conn.new_id();
    conn.send(MessageBuilder::new(xdg_surface, 1).uint(toplevel).finish())?;
    conn.send(
        MessageBuilder::new(toplevel, 2)
            .string(&format!("paint {color:06x}"))
            .finish(),
    )?;
    conn.send(MessageBuilder::new(surface, 6).finish())?;

    // 3. 컴포지터가 크기를 알려 줄 때마다(configure) 그 크기로 다시 그린다.
    let deadline = Instant::now() + Duration::from_secs(seconds.max(1));
    let mut size = (0, 0);
    let mut configure_serial = None;
    let mut ping = None;
    let mut buffer: Option<u32> = None;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        conn.read_events(Some(left), &mut |object, opcode, mut args| {
            if object == toplevel && opcode == 0 {
                let width = args.int().expect("width");
                let height = args.int().expect("height");
                size = (width, height);
            } else if object == xdg_surface && opcode == 0 {
                configure_serial = Some(args.uint().expect("serial"));
            } else if object == wm_base && opcode == 0 {
                ping = Some(args.uint().expect("serial"));
            }
        })?;
        if let Some(serial) = ping.take() {
            conn.send(MessageBuilder::new(wm_base, 3).uint(serial).finish())?;
        }
        let Some(serial) = configure_serial.take() else {
            continue;
        };
        // 0은 "네가 정해라"라는 뜻이다.
        let width = u32::try_from(size.0)
            .ok()
            .filter(|w| *w > 0)
            .unwrap_or(DEFAULT_WIDTH);
        let height = u32::try_from(size.1)
            .ok()
            .filter(|h| *h > 0)
            .unwrap_or(DEFAULT_HEIGHT);
        println!("configure {serial}: {width}x{height}");
        conn.send(MessageBuilder::new(xdg_surface, 4).uint(serial).finish())?;
        let next = conn.draw(shm, width, height, color)?;
        conn.send(
            MessageBuilder::new(surface, 1)
                .uint(next)
                .int(0)
                .int(0)
                .finish(),
        )?;
        conn.send(
            MessageBuilder::new(surface, 2)
                .int(0)
                .int(0)
                .int(width as i32)
                .int(height as i32)
                .finish(),
        )?;
        conn.send(MessageBuilder::new(surface, 6).finish())?;
        // 새 버퍼의 픽셀은 commit 때 컴포지터가 복사해 간다. 옛 버퍼는 버린다.
        if let Some(old) = buffer.replace(next) {
            conn.send(MessageBuilder::new(old, 0).finish())?;
        }
    }
    Ok(())
}
