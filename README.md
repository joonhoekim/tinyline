# tinyline

A tiny Wayland compositor written from scratch in Rust, one step at a time — the companion code for a Korean tutorial series.

Wayland 프로토콜을 라이브러리 없이 직접 구현하며 Rust를 처음부터 배우는 연재의 예제 코드다.
smithay 같은 라이브러리가 대신 해 주는 일 — 소켓을 열고, 바이트로 된 메시지를 읽고,
클라이언트가 공유 메모리로 넘긴 픽셀을 받아 겹친 창을 한 장으로 합치는 일 — 을 손으로 한 번 해 본다.

## 편별 태그

연재의 각 편이 끝난 상태가 태그로 남아 있다. 어느 편에서든 시작할 수 있다.

```sh
git clone https://github.com/joonhoekim/tinyline
cd tinyline
git checkout ep05
cargo run
```

| 태그 | 편 | 이 편에서 더한 것 |
|---|---|---|
| `ep01` | 화면 크기 찍기 | `cargo new`, 변수와 정수 타입 |
| `ep02` | 사각형 하나 | `Rect` 구조체, 겹침·포함 판정 |
| `ep03` | 창 목록 | `Window`·`Desktop`, 창 열고 닫기 |
| `ep04` | 화면을 나누기 | 창을 세로 기둥으로 나란히 배치 |
| `ep05` | 포커스 | 포커스된 창, 클릭으로 창 찾기 |
| `ep06` | 명령 받기 | 표준 입력으로 `open`/`close`/`focus`… |
| `ep07` | 테스트로 고정하기 | 배치·포커스·명령 해석 테스트 |
| `ep08` | 메시지 한 통 | Wayland 메시지 머리와 인자 읽고 쓰기 (`cargo run -- wire`) |
| `ep09` | 망가진 메시지 | `Result`와 에러 타입, "덜 왔다"와 "망가졌다" 구분 |
| `ep10` | 객체 표 | 객체 id 표, `sync`·`get_registry`·`bind`, 프로토콜 에러 (`cargo run -- protocol`) |
| `ep11` | 파일 나누기 | `lib.rs`와 모듈 |
| `ep12` | 소켓 열기 | 유닉스 소켓, 첫 진짜 클라이언트(`wayland-info`) |
| `ep13` | 여러 클라이언트 | `poll` 이벤트 루프, 첫 외부 크레이트 |
| `ep14` | 공유 메모리 | 소켓으로 fd 받기, `mmap`, 시험용 클라이언트 `examples/paint.rs` |
| `ep15` | 표면과 커밋 | `wl_surface`, 대기 상태와 현재 상태 |
| `ep16` | 한 장으로 합치기 | 합성, 알파 블렌딩, PPM 저장 |
| `ep17` | 창이 되기 | xdg-shell, 컴포지터가 정하는 창 크기 |
| `ep18` | 남이 만든 클라이언트 | weston 데모 클라이언트 띄우기 |

`ep06`부터는 명령을 파이프로 넘겨 볼 수 있다.

```sh
printf 'open terminal\nopen editor\nlist\nquit\n' | cargo run
```

`ep12`부터는 컴포지터를 띄우고 다른 터미널에서 클라이언트를 붙인다.

```sh
cargo run -- serve                                        # 터미널 1
WAYLAND_DISPLAY=tinyline-0 wayland-info                   # 터미널 2 (ep12~)
WAYLAND_DISPLAY=tinyline-0 cargo run --example paint -- 5 # 터미널 2 (ep14~)
```

`ep16`부터는 화면이 바뀔 때마다 `tinyline-frame.ppm`에 저장된다. `ep18`에서는 weston의
데모 클라이언트(`weston-terminal`, `weston-flower` 등)도 붙는다.

```sh
cargo run -- serve --quiet
WAYLAND_DISPLAY=tinyline-0 weston-terminal
```

## 준비물

- Rust (edition 2024 — `rustup`으로 설치한 stable이면 된다)
- 리눅스(`ep12`부터 유닉스 소켓, `ep14`부터 `memfd`를 쓴다). 실제 화면은 쓰지 않으므로 Wayland 세션이 없어도 된다

## 라이선스

MIT
