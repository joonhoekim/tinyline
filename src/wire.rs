//! Wayland 와이어 포맷: 메시지를 바이트로 쓰고 읽는다.

/// Wayland 메시지의 머리. 모든 메시지는 이 8바이트로 시작한다.
///
/// - 첫 4바이트: 이 메시지를 보낸(또는 받을) 객체의 id
/// - 다음 4바이트: 위 16비트는 머리를 포함한 메시지 전체 길이, 아래 16비트는 opcode
///
/// 숫자는 모두 이 기계의 바이트 순서(native endian)로 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub object: u32,
    pub opcode: u16,
    pub size: u16,
}

pub const HEADER_SIZE: usize = 8;

/// 메시지를 읽다가 생길 수 있는 문제들.
#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    /// 인자를 읽으려는데 바이트가 모자란다.
    Truncated { needed: usize, available: usize },
    /// 머리에 적힌 길이가 머리보다 짧거나 4의 배수가 아니다.
    BadSize(u16),
    /// 문자열의 길이가 0이거나, 끝에 0 바이트가 없다.
    BadString,
    /// 문자열이 UTF-8이 아니다.
    NotUtf8,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Truncated { needed, available } => {
                write!(f, "needed {needed} bytes but only {available} are left")
            }
            WireError::BadSize(size) => write!(f, "message size {size} is invalid"),
            WireError::BadString => write!(f, "string is not terminated by a 0 byte"),
            WireError::NotUtf8 => write!(f, "string is not valid UTF-8"),
        }
    }
}

impl std::error::Error for WireError {}

/// 4바이트를 숫자 하나로. 길이가 정확히 4인 조각만 받는다.
fn word(bytes: &[u8]) -> [u8; 4] {
    bytes.try_into().expect("caller passes exactly 4 bytes")
}

/// 바이트 앞부분에서 메시지 하나를 잘라 낸다.
///
/// - 메시지 하나가 다 도착했으면 `Ok(Some((머리, 인자 바이트)))`
/// - 아직 덜 왔으면 `Ok(None)` — 더 받은 뒤 다시 부르면 된다
/// - 머리부터 말이 안 되면 `Err`
pub fn split_message(bytes: &[u8]) -> Result<Option<(Header, &[u8])>, WireError> {
    if bytes.len() < HEADER_SIZE {
        return Ok(None);
    }
    let object = u32::from_ne_bytes(word(&bytes[0..4]));
    let packed = u32::from_ne_bytes(word(&bytes[4..8]));
    let header = Header {
        object,
        opcode: (packed & 0xffff) as u16,
        size: (packed >> 16) as u16,
    };
    let size = usize::from(header.size);
    if size < HEADER_SIZE || !size.is_multiple_of(4) {
        return Err(WireError::BadSize(header.size));
    }
    if bytes.len() < size {
        return Ok(None);
    }
    Ok(Some((header, &bytes[HEADER_SIZE..size])))
}

/// 메시지 하나를 바이트로 조립한다. 인자를 순서대로 붙인 뒤 `finish`로 머리를 씌운다.
pub struct MessageBuilder {
    object: u32,
    opcode: u16,
    args: Vec<u8>,
}

impl MessageBuilder {
    pub fn new(object: u32, opcode: u16) -> MessageBuilder {
        MessageBuilder {
            object,
            opcode,
            args: Vec::new(),
        }
    }

    pub fn uint(mut self, value: u32) -> MessageBuilder {
        self.args.extend_from_slice(&value.to_ne_bytes());
        self
    }

    pub fn int(mut self, value: i32) -> MessageBuilder {
        self.args.extend_from_slice(&value.to_ne_bytes());
        self
    }

    /// 문자열: 길이(끝의 0 바이트 포함) → 글자 → 0 바이트 → 4의 배수가 되도록 0으로 채움.
    pub fn string(mut self, value: &str) -> MessageBuilder {
        let length = u32::try_from(value.len() + 1).expect("string length fits in u32");
        self = self.uint(length);
        self.args.extend_from_slice(value.as_bytes());
        self.args.push(0);
        while !self.args.len().is_multiple_of(4) {
            self.args.push(0);
        }
        self
    }

    pub fn finish(self) -> Vec<u8> {
        let size = HEADER_SIZE + self.args.len();
        let size = u32::try_from(size).expect("message size fits in u32");
        let word = (size << 16) | u32::from(self.opcode);
        let mut bytes = Vec::with_capacity(HEADER_SIZE + self.args.len());
        bytes.extend_from_slice(&self.object.to_ne_bytes());
        bytes.extend_from_slice(&word.to_ne_bytes());
        bytes.extend_from_slice(&self.args);
        bytes
    }
}

/// 메시지의 인자 부분을 앞에서부터 하나씩 읽는다.
///
/// 읽는 동안 원래 바이트를 빌려 들고 있으므로, 그 바이트가 얼마나 살아 있는지를
/// 타입에 `'a`로 적는다.
pub struct Args<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Args<'a> {
    pub fn new(bytes: &'a [u8]) -> Args<'a> {
        Args { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], WireError> {
        let available = self.bytes.len() - self.position;
        if count > available {
            return Err(WireError::Truncated {
                needed: count,
                available,
            });
        }
        let chunk = &self.bytes[self.position..self.position + count];
        self.position += count;
        Ok(chunk)
    }

    pub fn uint(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_ne_bytes(word(self.take(4)?)))
    }

    pub fn int(&mut self) -> Result<i32, WireError> {
        Ok(i32::from_ne_bytes(word(self.take(4)?)))
    }

    pub fn string(&mut self) -> Result<&'a str, WireError> {
        let length = usize::try_from(self.uint()?).expect("u32 fits in usize");
        if length == 0 {
            return Err(WireError::BadString);
        }
        let padded = length.div_ceil(4) * 4;
        let chunk = self.take(padded)?;
        let (text, rest) = chunk.split_at(length - 1);
        if rest[0] != 0 {
            return Err(WireError::BadString);
        }
        std::str::from_utf8(text).map_err(|_| WireError::NotUtf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let bytes = MessageBuilder::new(7, 3).uint(42).finish();
        assert_eq!(bytes.len(), 12);
        let (header, body) = split_message(&bytes).unwrap().unwrap();
        assert_eq!(
            header,
            Header {
                object: 7,
                opcode: 3,
                size: 12
            }
        );
        assert_eq!(Args::new(body).uint(), Ok(42));
    }

    #[test]
    fn incomplete_messages_wait_for_more() {
        let bytes = MessageBuilder::new(7, 3).uint(42).finish();
        for cut in 0..bytes.len() {
            assert_eq!(split_message(&bytes[..cut]), Ok(None), "cut at {cut}");
        }
    }

    #[test]
    fn rejects_bad_sizes() {
        let mut bytes = MessageBuilder::new(1, 0).uint(0).finish();
        bytes[6] = 10; // 4의 배수가 아니다
        assert_eq!(split_message(&bytes), Err(WireError::BadSize(10)));
    }

    #[test]
    fn strings_are_padded_to_four_bytes() {
        for text in ["", "a", "abc", "abcd", "wl_compositor"] {
            let bytes = MessageBuilder::new(1, 0).string(text).uint(99).finish();
            assert_eq!(bytes.len() % 4, 0);
            let (_, body) = split_message(&bytes).unwrap().unwrap();
            let mut args = Args::new(body);
            assert_eq!(args.string(), Ok(text));
            assert_eq!(args.uint(), Ok(99));
        }
    }

    #[test]
    fn reading_past_the_end_fails() {
        let bytes = MessageBuilder::new(1, 0).uint(1).finish();
        let mut args = Args::new(&bytes[HEADER_SIZE..]);
        assert_eq!(args.uint(), Ok(1));
        assert_eq!(
            args.uint(),
            Err(WireError::Truncated {
                needed: 4,
                available: 0
            })
        );
    }

    #[test]
    fn rejects_bad_strings() {
        let mut bytes = MessageBuilder::new(1, 0).string("abc").finish();
        bytes[HEADER_SIZE + 4 + 3] = b'!'; // 끝의 0 바이트를 덮어쓴다
        assert_eq!(
            Args::new(&bytes[HEADER_SIZE..]).string(),
            Err(WireError::BadString)
        );
        let zero_length = 0u32.to_ne_bytes();
        assert_eq!(Args::new(&zero_length).string(), Err(WireError::BadString));
    }
}
