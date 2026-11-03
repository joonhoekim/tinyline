//! 표준 입력으로 받는 명령.

/// 사용자가 입력한 명령 한 줄.
#[derive(Debug, PartialEq)]
pub enum Command {
    Open(String),
    Close(u32),
    Focus(u32),
    Next,
    Click { x: i32, y: i32 },
    List,
    Help,
    Quit,
}

/// 명령 한 줄을 해석한다. 알아들을 수 없는 줄이면 `None`.
pub fn parse_command(line: &str) -> Option<Command> {
    let mut words = line.split_whitespace();
    let name = words.next()?;
    let command = match name {
        "open" => {
            let title: Vec<&str> = words.collect();
            if title.is_empty() {
                return None;
            }
            Command::Open(title.join(" "))
        }
        "close" => Command::Close(words.next()?.parse().ok()?),
        "focus" => Command::Focus(words.next()?.parse().ok()?),
        "next" => Command::Next,
        "click" => {
            let x = words.next()?.parse().ok()?;
            let y = words.next()?.parse().ok()?;
            Command::Click { x, y }
        }
        "list" => Command::List,
        "help" => Command::Help,
        "quit" | "exit" => Command::Quit,
        _ => return None,
    };
    Some(command)
}

pub const HELP: &str =
    "commands: open <title> | close <id> | focus <id> | next | click <x> <y> | list | help | quit";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(
            parse_command("open web browser"),
            Some(Command::Open("web browser".to_string()))
        );
        assert_eq!(parse_command("close 3"), Some(Command::Close(3)));
        assert_eq!(
            parse_command("click -5 10"),
            Some(Command::Click { x: -5, y: 10 })
        );
        assert_eq!(parse_command("exit"), Some(Command::Quit));
        assert_eq!(parse_command("open"), None);
        assert_eq!(parse_command("close x"), None);
        assert_eq!(parse_command("click 1"), None);
        assert_eq!(parse_command("fly"), None);
    }
}
