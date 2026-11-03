mod demo;

use tinyline::command::{Command, HELP, parse_command};
use tinyline::layout::{Desktop, Rect};

fn main() {
    // `cargo run -- wire`로 실행하면 메시지 형식을 보여 주고 끝난다.
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "wire" {
        if let Err(error) = demo::wire_demo() {
            println!("wire demo stopped: {error}");
        }
        return;
    }
    if mode == "protocol" {
        if let Err(error) = demo::protocol_demo() {
            println!("protocol demo stopped: {error}");
        }
        return;
    }

    let mut desktop = Desktop::new(Rect::new(0, 0, 1920, 1080));
    println!("{HELP}");

    for line in std::io::stdin().lines() {
        let line = line.expect("stdin is readable");
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(command) = parse_command(line) else {
            println!("? {line:?} (type help)");
            continue;
        };
        match command {
            Command::Open(title) => {
                let id = desktop.open(&title);
                println!("opened #{id}");
            }
            Command::Close(id) => {
                if !desktop.close(id) {
                    println!("no window #{id}");
                }
            }
            Command::Focus(id) => {
                if !desktop.focus(id) {
                    println!("no window #{id}");
                }
            }
            Command::Next => desktop.focus_next(),
            Command::Click { x, y } => match desktop.window_at(x, y) {
                Some(window) => {
                    let id = window.id;
                    desktop.focus(id);
                    println!("focused #{id}");
                }
                None => println!("nothing at ({x}, {y})"),
            },
            Command::List => desktop.print(),
            Command::Help => println!("{HELP}"),
            Command::Quit => break,
        }
        if let Some(window) = desktop.focused_window() {
            println!("[focus: #{} {}]", window.id, window.title);
        }
    }
}
