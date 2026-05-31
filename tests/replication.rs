use std::io;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use virting_ublk_drbd_demo::client;
use virting_ublk_drbd_demo::server::{
    PrimaryConfig, PrimaryState, SecondaryConfig, SecondaryState, serve_primary, serve_secondary,
};
use virting_ublk_drbd_demo::{DEFAULT_BLOCK_SIZE, mib};

#[test]
fn protocol_c_write_reaches_secondary_before_ack() -> io::Result<()> {
    let dir = temp_dir("protocol-c");
    let size = mib(8);

    let secondary = start_secondary(dir.join("secondary.img"), size)?;
    let primary = start_primary(
        dir.join("primary.img"),
        dir.join("primary.dirty"),
        secondary,
        size,
    )?;

    client::write(primary, 0, b"replicated")?;

    assert_eq!(client::read(primary, 0, 10)?, b"replicated");
    assert_eq!(client::read(secondary, 0, 10)?, b"replicated");
    assert_eq!(client::checksum(primary)?, client::checksum(secondary)?);
    assert!(client::status(primary)?.contains("dirty_blocks=0"));
    Ok(())
}

#[test]
fn peer_outage_marks_dirty_and_resync_replays_later() -> io::Result<()> {
    let dir = temp_dir("dirty-resync");
    let size = mib(8);

    let peer_addr = reserve_addr()?;
    let primary = start_primary_with_peer(
        dir.join("primary.img"),
        dir.join("primary.dirty"),
        peer_addr,
        size,
    )?;

    let err = client::write(primary, 100, b"late peer").unwrap_err();
    assert!(
        err.to_string()
            .contains("local write committed but peer replication failed"),
        "{err}"
    );
    assert!(client::status(primary)?.contains("dirty_blocks=1"));

    start_secondary_on(peer_addr, dir.join("secondary.img"), size)?;
    let replayed = client::resync(primary, false)?;

    assert_eq!(replayed, 1);
    assert_eq!(client::read(peer_addr, 100, 9)?, b"late peer");
    assert!(client::status(primary)?.contains("dirty_blocks=0"));
    Ok(())
}

fn start_secondary(backing: PathBuf, size: u64) -> io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    start_secondary_listener(listener, backing, size)?;
    Ok(addr)
}

fn start_secondary_on(addr: SocketAddr, backing: PathBuf, size: u64) -> io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    start_secondary_listener(listener, backing, size)
}

fn start_secondary_listener(listener: TcpListener, backing: PathBuf, size: u64) -> io::Result<()> {
    let addr = listener.local_addr()?;
    let state = Arc::new(SecondaryState::new(&SecondaryConfig {
        listen: addr,
        backing,
        size,
        block_size: DEFAULT_BLOCK_SIZE,
    })?);
    thread::spawn(move || {
        let _ = serve_secondary(listener, state);
    });
    thread::sleep(Duration::from_millis(50));
    Ok(())
}

fn start_primary(
    backing: PathBuf,
    bitmap: PathBuf,
    peer: SocketAddr,
    size: u64,
) -> io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let state = Arc::new(PrimaryState::new(&PrimaryConfig {
        listen: addr,
        peer,
        backing,
        bitmap,
        size,
        block_size: DEFAULT_BLOCK_SIZE,
    })?);
    thread::spawn(move || {
        let _ = serve_primary(listener, state);
    });
    thread::sleep(Duration::from_millis(50));
    Ok(addr)
}

fn start_primary_with_peer(
    backing: PathBuf,
    bitmap: PathBuf,
    peer: SocketAddr,
    size: u64,
) -> io::Result<SocketAddr> {
    start_primary(backing, bitmap, peer, size)
}

fn reserve_addr() -> io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.local_addr()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vrbd-test-{name}-{}-{}",
        std::process::id(),
        unix_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
