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

`ep06`부터는 명령을 파이프로 넘겨 볼 수 있다.

```sh
printf 'open terminal\nopen editor\nlist\nquit\n' | cargo run
```

## 준비물

- Rust (edition 2024 — `rustup`으로 설치한 stable이면 된다)
- 뒤쪽 편부터는 Wayland 세션이 도는 리눅스

## 라이선스

MIT
