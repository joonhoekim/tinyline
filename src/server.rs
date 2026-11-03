//! 유닉스 소켓을 열고, 여러 클라이언트와 동시에 바이트를 주고받는다.

use std::fs;
use std::io::{self, IoSliceMut, Write};
use std::mem::MaybeUninit;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rustix::event::{PollFd, PollFlags, poll};
use rustix::io::Errno;
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};

use crate::layout::Rect;
use crate::protocol::{Client, Shared};
use crate::render::Frame;

/// `$XDG_RUNTIME_DIR` 안에 만든 소켓 파일. 이 값이 사라질 때 파일도 지운다.
pub struct Socket {
    pub path: PathBuf,
    pub listener: UnixListener,
}

impl Socket {
    /// `$XDG_RUNTIME_DIR/<name>`에 소켓을 연다. 클라이언트는 `WAYLAND_DISPLAY=<name>`으로 찾아온다.
    pub fn bind(name: &str) -> io::Result<Socket> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set"))?;
        let path = PathBuf::from(dir).join(name);
        // 지난번에 비정상 종료해서 남은 파일이 있으면 지운다.
        if path.exists() {
            fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        Ok(Socket { path, listener })
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        // 지우다 실패해도 할 수 있는 일이 없으니 결과는 버린다.
        let _ = fs::remove_file(&self.path);
    }
}

/// 연결 하나: 소켓, 그 클라이언트의 프로토콜 상태, 아직 처리하지 못한 바이트.
struct Connection {
    stream: UnixStream,
    client: Client,
    pending: Vec<u8>,
    closed: bool,
}

impl Connection {
    /// 읽을 것이 있다고 poll이 알려 준 연결에서 한 번 읽고, 처리하고, 답을 보낸다.
    ///
    /// 바이트와 함께 파일 디스크립터가 올 수 있으므로 `read` 대신 `recvmsg`로 읽는다.
    fn on_readable(&mut self, shared: &mut Shared) -> io::Result<()> {
        let mut chunk = [0u8; 4096];
        // 메시지 한 번에 fd가 최대 28개까지 올 수 있다(libwayland가 한 번에 보내는 최대치).
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(28))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let received = recvmsg(
            &self.stream,
            &mut [IoSliceMut::new(&mut chunk)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC,
        );
        let count = match received {
            Ok(message) => message.bytes,
            Err(Errno::AGAIN) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for message in control.drain() {
            if let RecvAncillaryMessage::ScmRights(fds) = message {
                self.client.fds.extend(fds);
            }
        }
        if count == 0 {
            println!("[{}] hung up", self.client.id);
            self.closed = true;
            return Ok(());
        }
        self.pending.extend_from_slice(&chunk[..count]);

        match self.client.receive(&self.pending, shared) {
            Ok(used) => {
                self.pending.drain(..used);
            }
            Err(error) => {
                println!("[{}] protocol error: {error}", self.client.id);
                self.client.send_error(&error);
                self.closed = true;
            }
        }
        self.flush()
    }

    /// 쌓인 이벤트를 소켓으로 보낸다.
    fn flush(&mut self) -> io::Result<()> {
        self.stream.write_all(&self.client.outgoing)?;
        self.client.outgoing.clear();
        Ok(())
    }
}

const BACKGROUND: u32 = 0xff_2e_34_40;
const FOCUS_BORDER: u32 = 0xff_88_c0_d0;

/// 창마다 그 표면의 그림을 찾아 출력 한 장으로 합친다.
fn composite(shared: &Shared, connections: &[Connection]) -> Frame {
    let output = shared.desktop.output();
    let mut frame = Frame::new(output.width, output.height, BACKGROUND);
    for window in shared.desktop.windows() {
        let Some((client_id, surface_id)) = shared.windows.get(&window.id) else {
            continue;
        };
        let image = connections
            .iter()
            .find(|connection| connection.client.id == *client_id)
            .and_then(|connection| connection.client.surfaces.get(surface_id))
            .and_then(|surface| surface.image.as_ref());
        if let Some(image) = image {
            frame.draw(image, window.rect);
        }
        if shared.desktop.focused() == Some(window.id) {
            frame.outline(window.rect, 2, FOCUS_BORDER);
        }
    }
    frame
}

/// 소켓 하나와 연결된 클라이언트들을 돌보는 이벤트 루프. 돌아오지 않는다.
///
/// `log`를 끄면 메시지마다 찍는 로그를 생략한다(진짜 클라이언트는 메시지를 아주 많이 보낸다).
pub fn run(socket: &Socket, output: Rect, frame_path: &Path, log: bool) -> io::Result<()> {
    socket.listener.set_nonblocking(true)?;
    let mut shared = Shared::new(output);
    let mut frames = 0;
    // 마지막으로 보여 준 창 목록(제목). 바뀌었을 때만 다시 찍는다.
    let mut shown: Vec<String> = Vec::new();
    let mut connections: Vec<Connection> = Vec::new();
    let mut next_id = 1;
    let started = Instant::now();

    loop {
        // 1. 무엇이 준비됐는지 묻는다. 첫 칸은 새 접속, 나머지는 연결마다 하나씩.
        let mut fds = Vec::with_capacity(1 + connections.len());
        fds.push(PollFd::new(&socket.listener, PollFlags::IN));
        for connection in &connections {
            fds.push(PollFd::new(&connection.stream, PollFlags::IN));
        }
        poll(&mut fds, None)?;

        // 2. 결과만 따로 옮겨 적는다. `fds`는 연결들을 빌려 들고 있어서,
        //    이게 살아 있는 동안에는 연결을 고칠 수 없다.
        let ready: Vec<bool> = fds.iter().map(|fd| !fd.revents().is_empty()).collect();
        drop(fds);

        // 3. 새 접속을 받는다. 논블로킹이므로 더 없으면 WouldBlock이 온다.
        if ready[0] {
            loop {
                match socket.listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(true)?;
                        let mut client = Client::new(next_id);
                        client.log = log;
                        println!("[{next_id}] connected");
                        next_id += 1;
                        connections.push(Connection {
                            stream,
                            client,
                            pending: Vec::new(),
                            closed: false,
                        });
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error),
                }
            }
        }

        // 4. 읽을 것이 있는 연결을 처리한다. 방금 받은 연결은 `ready`에 칸이 없다.
        for (connection, is_ready) in connections.iter_mut().zip(&ready[1..]) {
            if *is_ready && let Err(error) = connection.on_readable(&mut shared) {
                println!("[{}] i/o error: {error}", connection.client.id);
                connection.closed = true;
            }
        }

        // 5. 끝난 연결을 치운다. 그 클라이언트의 창도 닫는다. 연결이 사라지면 소켓도 닫힌다.
        for connection in &connections {
            if connection.closed {
                shared.forget_client(connection.client.id);
            }
        }
        connections.retain(|connection| !connection.closed);

        // 6. 바뀐 것이 있으면 다시 그리고, 그렸다고 클라이언트들에게 알린다.
        if shared.dirty {
            shared.dirty = false;
            // 배치가 바뀌었을 수 있다. 창마다 새 크기와 포커스를 알린다.
            for connection in &mut connections {
                connection.client.configure_toplevels(&mut shared);
            }
            let frame = composite(&shared, &connections);
            frame.save_ppm(frame_path)?;
            frames += 1;
            if log {
                println!(
                    "frame {frames}: {} window(s) -> {}",
                    shared.desktop.windows().len(),
                    frame_path.display()
                );
            }
            let titles: Vec<String> = shared
                .desktop
                .windows()
                .iter()
                .map(|window| window.title.clone())
                .collect();
            if titles != shown {
                shared.desktop.print();
                shown = titles;
            }
            let time_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
            for connection in &mut connections {
                connection.client.frame_done(time_ms);
                if connection.flush().is_err() {
                    connection.closed = true;
                }
            }
        }
    }
}
