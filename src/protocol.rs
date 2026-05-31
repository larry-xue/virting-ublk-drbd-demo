use std::fmt;
use std::io::{self, Read, Write};

pub const MAGIC: [u8; 8] = *b"VRBDMVP1";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 40;
pub const MAX_PAYLOAD_LEN: u32 = 16 * 1024 * 1024;

pub const FLAG_FULL_RESYNC: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Op {
    Hello = 1,
    HelloOk = 2,
    Write = 3,
    WriteOk = 4,
    Read = 5,
    ReadOk = 6,
    Status = 7,
    StatusOk = 8,
    Resync = 9,
    ResyncOk = 10,
    Checksum = 11,
    ChecksumOk = 12,
    Error = 13,
}

impl TryFrom<u16> for Op {
    type Error = io::Error;

    fn try_from(value: u16) -> io::Result<Self> {
        match value {
            1 => Ok(Self::Hello),
            2 => Ok(Self::HelloOk),
            3 => Ok(Self::Write),
            4 => Ok(Self::WriteOk),
            5 => Ok(Self::Read),
            6 => Ok(Self::ReadOk),
            7 => Ok(Self::Status),
            8 => Ok(Self::StatusOk),
            9 => Ok(Self::Resync),
            10 => Ok(Self::ResyncOk),
            11 => Ok(Self::Checksum),
            12 => Ok(Self::ChecksumOk),
            13 => Ok(Self::Error),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown op code {value}"),
            )),
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub op: Op,
    pub flags: u32,
    pub offset: u64,
    pub value: u64,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(op: Op) -> Self {
        Self {
            op,
            flags: 0,
            offset: 0,
            value: 0,
            payload: Vec::new(),
        }
    }

    pub fn with_payload(op: Op, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            payload: payload.into(),
            ..Self::new(op)
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::with_payload(Op::Error, message.into().into_bytes())
    }

    pub fn payload_text(&self) -> String {
        String::from_utf8_lossy(&self.payload).into_owned()
    }
}

pub fn write_frame(mut writer: impl Write, frame: &Frame) -> io::Result<()> {
    if frame.payload.len() > MAX_PAYLOAD_LEN as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "payload too large: {} > {}",
                frame.payload.len(),
                MAX_PAYLOAD_LEN
            ),
        ));
    }

    let mut header = [0u8; HEADER_LEN];
    header[0..8].copy_from_slice(&MAGIC);
    header[8..10].copy_from_slice(&VERSION.to_be_bytes());
    header[10..12].copy_from_slice(&(frame.op as u16).to_be_bytes());
    header[12..16].copy_from_slice(&frame.flags.to_be_bytes());
    header[16..24].copy_from_slice(&frame.offset.to_be_bytes());
    header[24..32].copy_from_slice(&frame.value.to_be_bytes());
    header[32..36].copy_from_slice(&(frame.payload.len() as u32).to_be_bytes());
    writer.write_all(&header)?;
    writer.write_all(&frame.payload)?;
    writer.flush()
}

pub fn read_frame(mut reader: impl Read) -> io::Result<Frame> {
    let mut header = [0u8; HEADER_LEN];
    reader.read_exact(&mut header)?;

    if header[0..8] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid vrbd protocol magic",
        ));
    }

    let version = u16::from_be_bytes([header[8], header[9]]);
    if version != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported protocol version {version}"),
        ));
    }

    let op = Op::try_from(u16::from_be_bytes([header[10], header[11]]))?;
    let flags = u32::from_be_bytes([header[12], header[13], header[14], header[15]]);
    let offset = u64::from_be_bytes([
        header[16], header[17], header[18], header[19], header[20], header[21], header[22],
        header[23],
    ]);
    let value = u64::from_be_bytes([
        header[24], header[25], header[26], header[27], header[28], header[29], header[30],
        header[31],
    ]);
    let len = u32::from_be_bytes([header[32], header[33], header[34], header[35]]);
    if len > MAX_PAYLOAD_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("payload length {len} exceeds limit {MAX_PAYLOAD_LEN}"),
        ));
    }

    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload)?;

    Ok(Frame {
        op,
        flags,
        offset,
        value,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trips() {
        let frame = Frame {
            op: Op::Write,
            flags: FLAG_FULL_RESYNC,
            offset: 4096,
            value: 12,
            payload: b"hello world".to_vec(),
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &frame).unwrap();
        let decoded = read_frame(&buf[..]).unwrap();
        assert_eq!(decoded, frame);
    }
}
