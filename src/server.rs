use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::DEFAULT_BLOCK_SIZE;
use crate::backend::FileBackend;
use crate::bitmap::DirtyBitmap;
use crate::protocol::{FLAG_FULL_RESYNC, Frame, Op, read_frame, write_frame};

#[derive(Debug, Clone)]
pub struct SecondaryConfig {
    pub listen: SocketAddr,
    pub backing: PathBuf,
    pub size: u64,
    pub block_size: u64,
}

#[derive(Debug, Clone)]
pub struct PrimaryConfig {
    pub listen: SocketAddr,
    pub peer: SocketAddr,
    pub backing: PathBuf,
    pub bitmap: PathBuf,
    pub size: u64,
    pub block_size: u64,
}

#[derive(Debug)]
pub struct SecondaryState {
    backend: Arc<FileBackend>,
    size: u64,
    block_size: u64,
}

#[derive(Debug)]
pub struct PrimaryState {
    backend: Arc<FileBackend>,
    dirty: Mutex<DirtyBitmap>,
    peer: SocketAddr,
    size: u64,
    block_size: u64,
}

impl SecondaryState {
    pub fn new(config: &SecondaryConfig) -> io::Result<Self> {
        Ok(Self {
            backend: Arc::new(FileBackend::open(&config.backing, config.size)?),
            size: config.size,
            block_size: config.block_size,
        })
    }
}

impl PrimaryState {
    pub fn new(config: &PrimaryConfig) -> io::Result<Self> {
        Ok(Self {
            backend: Arc::new(FileBackend::open(&config.backing, config.size)?),
            dirty: Mutex::new(DirtyBitmap::load_or_create(
                &config.bitmap,
                config.block_size,
                config.size,
            )?),
            peer: config.peer,
            size: config.size,
            block_size: config.block_size,
        })
    }

    pub fn dirty_count(&self) -> io::Result<u64> {
        Ok(self.lock_dirty()?.dirty_count())
    }

    fn lock_dirty(&self) -> io::Result<std::sync::MutexGuard<'_, DirtyBitmap>> {
        self.dirty
            .lock()
            .map_err(|_| io::Error::other("dirty bitmap mutex poisoned"))
    }
}

pub fn run_secondary(config: SecondaryConfig) -> io::Result<()> {
    let listener = TcpListener::bind(config.listen)?;
    let state = Arc::new(SecondaryState::new(&config)?);
    println!(
        "secondary listening on {} backing={} size={}MiB",
        listener.local_addr()?,
        state.backend.path().display(),
        state.size / 1024 / 1024
    );
    serve_secondary(listener, state)
}

pub fn run_primary(config: PrimaryConfig) -> io::Result<()> {
    let listener = TcpListener::bind(config.listen)?;
    let state = Arc::new(PrimaryState::new(&config)?);
    println!(
        "primary listening on {} peer={} backing={} bitmap={}",
        listener.local_addr()?,
        config.peer,
        state.backend.path().display(),
        config.bitmap.display()
    );
    serve_primary(listener, state)
}

pub fn serve_secondary(listener: TcpListener, state: Arc<SecondaryState>) -> io::Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    if let Err(err) = handle_secondary_connection(stream, state) {
                        eprintln!("secondary connection error: {err}");
                    }
                });
            }
            Err(err) => eprintln!("secondary accept error: {err}"),
        }
    }
    Ok(())
}

pub fn serve_primary(listener: TcpListener, state: Arc<PrimaryState>) -> io::Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    if let Err(err) = handle_primary_connection(stream, state) {
                        eprintln!("primary connection error: {err}");
                    }
                });
            }
            Err(err) => eprintln!("primary accept error: {err}"),
        }
    }
    Ok(())
}

fn handle_secondary_connection(
    mut stream: TcpStream,
    state: Arc<SecondaryState>,
) -> io::Result<()> {
    handshake(&mut stream, state.size, state.block_size)?;
    let request = read_frame(&mut stream)?;
    let response = match request.op {
        Op::Write => match state.backend.write_at(request.offset, &request.payload) {
            Ok(()) => {
                let mut frame = Frame::new(Op::WriteOk);
                frame.value = request.payload.len() as u64;
                frame
            }
            Err(err) => Frame::error(err.to_string()),
        },
        Op::Read => match state
            .backend
            .read_at(request.offset, request.value as usize)
        {
            Ok(data) => Frame::with_payload(Op::ReadOk, data),
            Err(err) => Frame::error(err.to_string()),
        },
        Op::Status => {
            let text = format!(
                "role=secondary size={} block_size={} backing={}",
                state.size,
                state.block_size,
                state.backend.path().display()
            );
            Frame::with_payload(Op::StatusOk, text.into_bytes())
        }
        Op::Checksum => match state.backend.checksum() {
            Ok(sum) => {
                let mut frame = Frame::new(Op::ChecksumOk);
                frame.value = sum;
                frame
            }
            Err(err) => Frame::error(err.to_string()),
        },
        _ => Frame::error(format!("secondary does not handle {}", request.op)),
    };
    write_frame(&mut stream, &response)
}

fn handle_primary_connection(mut stream: TcpStream, state: Arc<PrimaryState>) -> io::Result<()> {
    handshake(&mut stream, state.size, state.block_size)?;
    let request = read_frame(&mut stream)?;
    let response = match request.op {
        Op::Write => handle_primary_write(&state, request.offset, &request.payload),
        Op::Read => match state
            .backend
            .read_at(request.offset, request.value as usize)
        {
            Ok(data) => Frame::with_payload(Op::ReadOk, data),
            Err(err) => Frame::error(err.to_string()),
        },
        Op::Resync => handle_resync(&state, (request.flags & FLAG_FULL_RESYNC) != 0),
        Op::Status => match state.dirty_count() {
            Ok(dirty) => {
                let text = format!(
                    "role=primary peer={} size={} block_size={} dirty_blocks={} backing={}",
                    state.peer,
                    state.size,
                    state.block_size,
                    dirty,
                    state.backend.path().display()
                );
                Frame::with_payload(Op::StatusOk, text.into_bytes())
            }
            Err(err) => Frame::error(err.to_string()),
        },
        Op::Checksum => match state.backend.checksum() {
            Ok(sum) => {
                let mut frame = Frame::new(Op::ChecksumOk);
                frame.value = sum;
                frame
            }
            Err(err) => Frame::error(err.to_string()),
        },
        _ => Frame::error(format!("primary does not handle {}", request.op)),
    };
    write_frame(&mut stream, &response)
}

fn handle_primary_write(state: &PrimaryState, offset: u64, data: &[u8]) -> Frame {
    if let Err(err) = state.backend.write_at(offset, data) {
        return Frame::error(err.to_string());
    }

    match replicate_write(state.peer, state.size, state.block_size, offset, data) {
        Ok(()) => match state.lock_dirty() {
            Ok(mut dirty) => {
                dirty.clear_range(offset, data.len() as u64);
                if let Err(err) = dirty.save() {
                    return Frame::error(err.to_string());
                }
                let mut frame = Frame::new(Op::WriteOk);
                frame.value = data.len() as u64;
                frame
            }
            Err(err) => Frame::error(err.to_string()),
        },
        Err(err) => match state.lock_dirty() {
            Ok(mut dirty) => {
                dirty.mark_range(offset, data.len() as u64);
                let save_result = dirty.save();
                let suffix = save_result
                    .err()
                    .map(|save_err| format!("; dirty bitmap save also failed: {save_err}"))
                    .unwrap_or_default();
                Frame::error(format!(
                    "local write committed but peer replication failed: {err}{suffix}"
                ))
            }
            Err(lock_err) => Frame::error(format!(
                "local write committed but peer replication failed: {err}; {lock_err}"
            )),
        },
    }
}

fn handle_resync(state: &PrimaryState, full: bool) -> Frame {
    let block_size = state.block_size;
    let blocks = match state.lock_dirty() {
        Ok(mut dirty) => {
            if full {
                dirty.mark_all();
            }
            dirty.dirty_blocks()
        }
        Err(err) => return Frame::error(err.to_string()),
    };

    let mut synced = 0u64;
    for block in blocks {
        let offset = block * block_size;
        let len = block_size.min(state.size - offset) as usize;
        let data = match state.backend.read_at(offset, len) {
            Ok(data) => data,
            Err(err) => return Frame::error(err.to_string()),
        };
        if let Err(err) = replicate_write(state.peer, state.size, state.block_size, offset, &data) {
            return Frame::error(format!(
                "resync stopped at block {block} offset {offset}: {err}"
            ));
        }
        synced += 1;

        match state.lock_dirty() {
            Ok(mut dirty) => {
                dirty.clear_range(offset, len as u64);
                if let Err(err) = dirty.save() {
                    return Frame::error(err.to_string());
                }
            }
            Err(err) => return Frame::error(err.to_string()),
        }
    }

    let mut frame = Frame::new(Op::ResyncOk);
    frame.value = synced;
    frame
}

pub fn replicate_write(
    peer: SocketAddr,
    size: u64,
    block_size: u64,
    offset: u64,
    data: &[u8],
) -> io::Result<()> {
    let mut stream = TcpStream::connect_timeout(&peer, Duration::from_secs(2))?;
    stream.set_nodelay(true)?;

    let mut hello = Frame::with_payload(Op::Hello, b"role=primary".to_vec());
    hello.offset = block_size;
    hello.value = size;
    write_frame(&mut stream, &hello)?;
    expect_response(read_frame(&mut stream)?, Op::HelloOk)?;

    let mut write = Frame::with_payload(Op::Write, data.to_vec());
    write.offset = offset;
    write.value = data.len() as u64;
    write_frame(&mut stream, &write)?;
    expect_response(read_frame(&mut stream)?, Op::WriteOk)?;
    Ok(())
}

fn handshake(stream: &mut TcpStream, size: u64, block_size: u64) -> io::Result<()> {
    let hello = read_frame(&mut *stream)?;
    if hello.op != Op::Hello {
        write_frame(&mut *stream, &Frame::error("first frame must be Hello"))?;
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "first frame must be Hello",
        ));
    }

    if hello.value != 0 && hello.value != size {
        let error = Frame::error(format!(
            "device size mismatch: peer={} local={}",
            hello.value, size
        ));
        write_frame(&mut *stream, &error)?;
        return Ok(());
    }
    if hello.offset != 0 && hello.offset != block_size && hello.offset != DEFAULT_BLOCK_SIZE {
        let error = Frame::error(format!(
            "block size mismatch: peer={} local={}",
            hello.offset, block_size
        ));
        write_frame(&mut *stream, &error)?;
        return Ok(());
    }

    let mut ok = Frame::new(Op::HelloOk);
    ok.offset = block_size;
    ok.value = size;
    write_frame(&mut *stream, &ok)
}

fn expect_response(frame: Frame, expected: Op) -> io::Result<Frame> {
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
