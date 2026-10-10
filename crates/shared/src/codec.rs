//! A tiny length-prefixed postcard transport suitable for TCP and loopback tests.

use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Read, Write};

pub const MAX_FRAME_SIZE: usize = 1024 * 1024;

#[derive(Debug)]
pub enum CodecError {
    Io(io::Error),
    InvalidLength(u32),
    Truncated { expected: usize, actual: usize },
    Postcard(postcard::Error),
}

impl From<io::Error> for CodecError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<postcard::Error> for CodecError {
    fn from(value: postcard::Error) -> Self {
        Self::Postcard(value)
    }
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::InvalidLength(length) => write!(f, "invalid frame length: {length}"),
            Self::Truncated { expected, actual } => write!(
                f,
                "truncated frame: expected {expected} bytes, got {actual}"
            ),
            Self::Postcard(error) => write!(f, "postcard error: {error}"),
        }
    }
}

impl std::error::Error for CodecError {}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let payload = postcard::to_allocvec(value)?;
    if payload.is_empty() || payload.len() > MAX_FRAME_SIZE {
        return Err(CodecError::InvalidLength(payload.len() as u32));
    }
    let length = payload.len() as u32;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame<T: DeserializeOwned>(bytes: &[u8]) -> Result<(T, usize), CodecError> {
    if bytes.len() < 4 {
        return Err(CodecError::Truncated {
            expected: 4,
            actual: bytes.len(),
        });
    }
    let length = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if length == 0 || length > MAX_FRAME_SIZE {
        return Err(CodecError::InvalidLength(length as u32));
    }
    let end = 4 + length;
    if bytes.len() < end {
        return Err(CodecError::Truncated {
            expected: end,
            actual: bytes.len(),
        });
    }
    Ok((postcard::from_bytes(&bytes[4..end])?, end))
}

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), CodecError> {
    writer.write_all(&encode_frame(value)?)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<T, CodecError> {
    let mut header = [0_u8; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_SIZE {
        return Err(CodecError::InvalidLength(length as u32));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    Ok(postcard::from_bytes(&payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ClientCommand, Command, Direction};
    use std::io::Cursor;

    #[test]
    fn round_trips_multiple_frames() {
        let first = ClientCommand::new(4, Command::Move(Direction::East));
        let second = ClientCommand::new(5, Command::Say("hello".into()));
        let mut bytes = encode_frame(&first).unwrap();
        bytes.extend(encode_frame(&second).unwrap());
        let (decoded, used) = decode_frame::<ClientCommand>(&bytes).unwrap();
        assert_eq!(decoded, first);
        let (decoded_again, _) = decode_frame::<ClientCommand>(&bytes[used..]).unwrap();
        assert_eq!(decoded_again, second);
    }

    #[test]
    fn stream_helpers_use_exact_length() {
        let command = ClientCommand::new(1, Command::Rest);
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &command).unwrap();
        assert_eq!(
            read_frame::<_, ClientCommand>(&mut Cursor::new(bytes)).unwrap(),
            command
        );
    }
}
