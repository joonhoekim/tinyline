//! 유닉스 소켓을 열고 클라이언트와 바이트를 주고받는다.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use crate::protocol::Client;

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

/// 클라이언트 하나를 받아, 연결이 끝날 때까지 대화한다.
pub fn serve_one(socket: &Socket) -> io::Result<()> {
    let (mut stream, _) = socket.listener.accept()?;
    println!("client connected");
    let mut client = Client::new();
    client.log = true;

    // 소켓에서 읽은 바이트 중 아직 처리하지 못한 것(덜 도착한 메시지)
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            println!("client hung up");
            return Ok(());
        }
        pending.extend_from_slice(&chunk[..count]);

        let result = client.receive(&pending);
        match result {
            Ok(used) => {
                pending.drain(..used);
            }
            Err(ref error) => {
                println!("protocol error: {error}");
                client.send_error(error);
            }
        }
        stream.write_all(&client.outgoing)?;
        client.outgoing.clear();
        if result.is_err() {
            return Ok(());
        }
    }
}
