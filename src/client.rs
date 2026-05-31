use std::io;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::DEFAULT_BLOCK_SIZE;
use crate::protocol::{FLAG_FULL_RESYNC, Frame, Op, read_frame, write_frame};

pub fn write(target: SocketAddr, offset: u64, data: &[u8]) -> io::Result<()> {
    let mut frame = Frame::with_payload(Op::Write, data.to_vec());
    frame.offset = offset;
    frame.value = data.len() as u64;
    expect_ok(request(target, frame)?, Op::WriteOk).map(|_| ())
}

pub fn read(target: SocketAddr, offset: u64, len: u64) -> io::Result<Vec<u8>> {
    let mut frame = Frame::new(Op::Read);
    frame.offset = offset;
    frame.value = len;
    let response = expect_ok(request(target, frame)?, Op::ReadOk)?;
    Ok(response.payload)
}

pub fn resync(target: SocketAddr, full: bool) -> io::Result<u64> {
    let mut frame = Frame::new(Op::Resync);
    if full {
        frame.flags |= FLAG_FULL_RESYNC;
    }
    let response = expect_ok(request(target, frame)?, Op::ResyncOk)?;
    Ok(response.value)
}

pub fn status(target: SocketAddr) -> io::Result<String> {
    let response = expect_ok(request(target, Frame::new(Op::Status))?, Op::StatusOk)?;
    Ok(response.payload_text())
}

pub fn checksum(target: SocketAddr) -> io::Result<u64> {
    let response = expect_ok(request(target, Frame::new(Op::Checksum))?, Op::ChecksumOk)?;
    Ok(response.value)
}

fn request(target: SocketAddr, frame: Frame) -> io::Result<Frame> {
    let mut stream = TcpStream::connect_timeout(&target, Duration::from_secs(2))?;
    stream.set_nodelay(true)?;

    let mut hello = Frame::with_payload(Op::Hello, b"role=client".to_vec());
    hello.offset = DEFAULT_BLOCK_SIZE;
    write_frame(&mut stream, &hello)?;
    let response = read_frame(&mut stream)?;
    expect_ok(response, Op::HelloOk)?;

    write_frame(&mut stream, &frame)?;
    read_frame(&mut stream)
}

fn expect_ok(frame: Frame, expected: Op) -> io::Result<Frame> {
    if frame.op == expected {
        Ok(frame)
    } else if frame.op == Op::Error {
        Err(io::Error::other(frame.payload_text()))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected {expected}, got {}", frame.op),
        ))
    }
}
