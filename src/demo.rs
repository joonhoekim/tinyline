//! `cargo run -- wire` · `cargo run -- protocol`로 보는 시연. 실행 파일에만 있다.

use tinyline::protocol::Client;
use tinyline::wire::{Args, HEADER_SIZE, Header, MessageBuilder, WireError, split_message};

/// 바이트를 4개씩 끊어 16진수로 보여 준다.
fn hex(bytes: &[u8]) -> String {
    let words: Vec<String> = bytes
        .chunks(4)
        .map(|word| word.iter().map(|b| format!("{b:02x}")).collect())
        .collect();
    words.join(" ")
}

pub fn wire_demo() -> Result<(), WireError> {
    // 클라이언트가 접속하자마자 보내는 두 요청
    // wl_display(객체 1).get_registry(새 객체 2)  — opcode 1
    // wl_display(객체 1).sync(새 객체 3)          — opcode 0
    let get_registry = MessageBuilder::new(1, 1).uint(2).finish();
    let sync = MessageBuilder::new(1, 0).uint(3).finish();
    println!("get_registry: {}", hex(&get_registry));
    println!("sync:         {}", hex(&sync));

    // 두 메시지가 이어 붙어 왔고, 세 번째 메시지는 앞 5바이트만 도착했다고 치자.
    let mut stream = get_registry.clone();
    stream.extend_from_slice(&sync);
    stream.extend_from_slice(&sync[..5]);
    let mut rest = stream.as_slice();
    while let Some((header, body)) = split_message(rest)? {
        let new_id = Args::new(body).uint()?;
        println!("{header:?} new_id={new_id}");
        rest = &rest[usize::from(header.size)..];
    }
    println!("waiting for {} more byte(s)", HEADER_SIZE - rest.len());

    // 망가진 메시지들
    let too_small = MessageBuilder::new(1, 0).finish();
    let mut bad_size = too_small.clone();
    bad_size[6] = 6; // 길이를 6으로: 머리(8바이트)보다 짧다
    println!("bad size:   {:?}", split_message(&bad_size));

    let short = MessageBuilder::new(1, 0).finish();
    let (_, body) = split_message(&short)?.expect("complete");
    match Args::new(body).uint() {
        Ok(value) => println!("got {value}"),
        Err(error) => println!("short body: {error}"),
    }

    let mut not_utf8 = MessageBuilder::new(2, 0).string("ab").finish();
    not_utf8[12] = 0xff;
    let (_, body) = split_message(&not_utf8)?.expect("complete");
    if let Err(error) = Args::new(body).string() {
        println!("bad string: {error}");
    }

    // 음수 좌표도 같은 4바이트에 담긴다
    let attach = MessageBuilder::new(5, 1).uint(0).int(-20).int(10).finish();
    let (_, body) = split_message(&attach)?.expect("complete");
    let mut args = Args::new(body);
    let (buffer, x, y) = (args.uint()?, args.int()?, args.int()?);
    println!("attach: buffer={buffer} x={x} y={y}");

    // ?는 에러를 만나면 그 자리에서 함수를 끝내고 에러를 호출한 쪽으로 돌려준다.
    let (_, body) = split_message(&short)?.expect("complete");
    let value = Args::new(body).uint()?;
    println!("never printed: {value}");
    Ok(())
}

/// 컴포지터가 보낸 이벤트를 사람이 읽을 수 있게 한 줄로. (대본용 — 객체 번호를 알고 있다고 친다.)
fn describe_event(header: Header, body: &[u8]) -> Result<String, WireError> {
    let mut args = Args::new(body);
    let text = match (header.object, header.opcode) {
        (1, 0) => format!(
            "wl_display.error(object {}, code {}, {:?})",
            args.uint()?,
            args.uint()?,
            args.string()?
        ),
        (1, 1) => format!("wl_display.delete_id({})", args.uint()?),
        (2, 0) => format!(
            "wl_registry.global({}, {:?}, {})",
            args.uint()?,
            args.string()?,
            args.uint()?
        ),
        (id, 0) => format!("wl_callback#{id}.done({})", args.uint()?),
        (id, opcode) => format!("object {id} event {opcode}"),
    };
    Ok(text)
}

fn print_events(bytes: &[u8]) -> Result<(), WireError> {
    let mut rest = bytes;
    while let Some((header, body)) = split_message(rest)? {
        println!("  <- {}", describe_event(header, body)?);
        rest = &rest[usize::from(header.size)..];
    }
    Ok(())
}

pub fn protocol_demo() -> Result<(), WireError> {
    let mut client = Client::new();

    // 클라이언트가 보낸다고 치는 요청들: get_registry(2), sync(3), bind(1, wl_compositor, 4, id 4)
    let mut requests = MessageBuilder::new(1, 1).uint(2).finish();
    requests.extend(MessageBuilder::new(1, 0).uint(3).finish());
    requests.extend(
        MessageBuilder::new(2, 0)
            .uint(1)
            .string("wl_compositor")
            .uint(4)
            .uint(4)
            .finish(),
    );
    println!("-> get_registry(2), sync(3), bind(1, \"wl_compositor\", 4, id 4)");
    match client.receive(&requests) {
        Ok(used) => println!("  handled {used} bytes"),
        Err(error) => println!("  error: {error}"),
    }
    print_events(&std::mem::take(&mut client.outgoing))?;

    // HashMap은 순서를 지키지 않으므로, 보여 줄 때는 id 순으로 정렬한다.
    let mut objects: Vec<(u32, &str)> = client
        .objects
        .iter()
        .map(|(id, object)| (*id, object.interface()))
        .collect();
    objects.sort_by_key(|(id, _)| *id);
    println!("objects: {objects:?}");

    // 규칙 위반: 없는 객체 9에 메시지를 보낸다
    println!("-> object 9, opcode 0");
    let bad = MessageBuilder::new(9, 0).finish();
    if let Err(error) = client.receive(&bad) {
        client.send_error(&error);
    }
    print_events(&std::mem::take(&mut client.outgoing))?;

    // 규칙 위반: 이미 있는 id 2로 레지스트리를 또 만든다
    println!("-> get_registry(2) again");
    let again = MessageBuilder::new(1, 1).uint(2).finish();
    if let Err(error) = client.receive(&again) {
        client.send_error(&error);
    }
    print_events(&std::mem::take(&mut client.outgoing))?;
    Ok(())
}
