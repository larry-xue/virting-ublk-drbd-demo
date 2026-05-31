use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::client;
use crate::server::{
    PrimaryConfig, PrimaryState, SecondaryConfig, SecondaryState, run_primary, run_secondary,
    serve_primary, serve_secondary,
};
use crate::{DEFAULT_BLOCK_SIZE, mib};

pub fn run() -> i32 {
    match run_inner(std::env::args().skip(1).collect()) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

fn run_inner(args: Vec<String>) -> io::Result<()> {
    let mut args = VecDeque::from(args);
    let Some(command) = args.pop_front() else {
        print_help();
        return Ok(());
    };

    match command.as_str() {
        "secondary" => {
            let flags = Flags::parse(args)?;
            let config = SecondaryConfig {
                listen: flags.required("listen")?.parse_addr("listen")?,
                backing: flags.required("backing")?.into(),
                size: mib(flags.optional_u64("size-mib")?.unwrap_or(64)),
                block_size: flags
                    .optional_u64("block-size")?
                    .unwrap_or(DEFAULT_BLOCK_SIZE),
            };
            run_secondary(config)
        }
        "primary" => {
            let flags = Flags::parse(args)?;
            let backing: PathBuf = flags.required("backing")?.into();
            let bitmap = flags
                .optional("bitmap")
                .map(PathBuf::from)
                .unwrap_or_else(|| backing.with_extension("dirty"));
            let config = PrimaryConfig {
                listen: flags.required("listen")?.parse_addr("listen")?,
                peer: flags.required("peer")?.parse_addr("peer")?,
                backing,
                bitmap,
                size: mib(flags.optional_u64("size-mib")?.unwrap_or(64)),
                block_size: flags
                    .optional_u64("block-size")?
                    .unwrap_or(DEFAULT_BLOCK_SIZE),
            };
            run_primary(config)
        }
        "write" => {
            let flags = Flags::parse(args)?;
            let target = flags.required("target")?.parse_addr("target")?;
            let offset = flags.optional_u64("offset")?.unwrap_or(0);
            let data = flags.required("data")?;
            client::write(target, offset, data.as_bytes())
        }
        "read" => {
            let flags = Flags::parse(args)?;
            let target = flags.required("target")?.parse_addr("target")?;
            let offset = flags.optional_u64("offset")?.unwrap_or(0);
            let len = flags.required("len")?.parse_u64("len")?;
            let data = client::read(target, offset, len)?;
            println!("hex={}", hex(&data));
            println!("utf8={}", String::from_utf8_lossy(&data));
            Ok(())
        }
        "resync" => {
            let flags = Flags::parse(args)?;
            let target = flags.required("target")?.parse_addr("target")?;
            let full = flags.has("full");
            let count = client::resync(target, full)?;
            println!("resynced_blocks={count}");
            Ok(())
        }
        "status" => {
            let flags = Flags::parse(args)?;
            let target = flags.required("target")?.parse_addr("target")?;
            println!("{}", client::status(target)?);
            Ok(())
        }
        "checksum" => {
            let flags = Flags::parse(args)?;
            let target = flags.required("target")?.parse_addr("target")?;
            println!("{:016x}", client::checksum(target)?);
            Ok(())
        }
        "demo" => {
            let flags = Flags::parse(args)?;
            let dir = flags.optional("dir").map(PathBuf::from).unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "vrbd-demo-{}-{}",
                    std::process::id(),
                    unix_nanos()
                ))
            });
            run_demo(dir)
        }
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown command `{other}`"),
        )),
    }
}

fn run_demo(dir: PathBuf) -> io::Result<()> {
    std::fs::create_dir_all(&dir)?;
    let size = mib(8);

    let secondary_listener = TcpListener::bind("127.0.0.1:0")?;
    let secondary_addr = secondary_listener.local_addr()?;
    let secondary = Arc::new(SecondaryState::new(&SecondaryConfig {
        listen: secondary_addr,
        backing: dir.join("secondary.img"),
        size,
        block_size: DEFAULT_BLOCK_SIZE,
    })?);
    thread::spawn(move || {
        let _ = serve_secondary(secondary_listener, secondary);
    });

    let primary_listener = TcpListener::bind("127.0.0.1:0")?;
    let primary_addr = primary_listener.local_addr()?;
    let primary = Arc::new(PrimaryState::new(&PrimaryConfig {
        listen: primary_addr,
        peer: secondary_addr,
        backing: dir.join("primary.img"),
        bitmap: dir.join("primary.dirty"),
        size,
        block_size: DEFAULT_BLOCK_SIZE,
    })?);
    thread::spawn(move || {
        let _ = serve_primary(primary_listener, primary);
    });

    thread::sleep(Duration::from_millis(100));
    client::write(primary_addr, 0, b"hello replicated block device")?;
    let data = client::read(primary_addr, 0, 29)?;
    let peer_data = client::read(secondary_addr, 0, 29)?;
    let primary_sum = client::checksum(primary_addr)?;
    let secondary_sum = client::checksum(secondary_addr)?;

    println!("demo_dir={}", dir.display());
    println!("primary_addr={primary_addr}");
    println!("secondary_addr={secondary_addr}");
    println!("primary_read={}", String::from_utf8_lossy(&data));
    println!("secondary_read={}", String::from_utf8_lossy(&peer_data));
    println!("primary_checksum={primary_sum:016x}");
    println!("secondary_checksum={secondary_sum:016x}");
    println!("status={}", client::status(primary_addr)?);
    Ok(())
}

fn print_help() {
    println!(
        r#"virting-ublk-drbd-demo

Standalone DRBD-like replication demo. This does not create /dev/ublkbN yet.

Commands:
  secondary --listen ADDR --backing PATH [--size-mib N]
  primary   --listen ADDR --peer ADDR --backing PATH [--bitmap PATH] [--size-mib N]
  write     --target ADDR [--offset BYTES] --data TEXT
  read      --target ADDR [--offset BYTES] --len BYTES
  resync    --target ADDR [--full]
  status    --target ADDR
  checksum  --target ADDR
  demo      [--dir PATH]
"#
    );
}

#[derive(Default)]
struct Flags {
    values: std::collections::BTreeMap<String, String>,
    bools: std::collections::BTreeSet<String>,
}

impl Flags {
    fn parse(mut args: VecDeque<String>) -> io::Result<Self> {
        let mut flags = Self::default();
        while let Some(arg) = args.pop_front() {
            let Some(name) = arg.strip_prefix("--") else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unexpected positional argument `{arg}`"),
                ));
            };

            match name {
                "full" => {
                    flags.bools.insert(name.to_string());
                }
                _ => {
                    let Some(value) = args.pop_front() else {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("missing value for --{name}"),
                        ));
                    };
                    flags.values.insert(name.to_string(), value);
                }
            }
        }
        Ok(flags)
    }

    fn required(&self, name: &str) -> io::Result<String> {
        self.values.get(name).cloned().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("missing required flag --{name}"),
            )
        })
    }

    fn optional(&self, name: &str) -> Option<String> {
        self.values.get(name).cloned()
    }

    fn optional_u64(&self, name: &str) -> io::Result<Option<u64>> {
        self.values
            .get(name)
            .map(|value| value.parse_u64(name))
            .transpose()
    }

    fn has(&self, name: &str) -> bool {
        self.bools.contains(name)
    }
}

trait ParseExt {
    fn parse_addr(&self, flag: &str) -> io::Result<SocketAddr>;
    fn parse_u64(&self, flag: &str) -> io::Result<u64>;
}

impl ParseExt for str {
    fn parse_addr(&self, flag: &str) -> io::Result<SocketAddr> {
        self.parse().map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid --{flag} socket address `{self}`: {err}"),
            )
        })
    }

    fn parse_u64(&self, flag: &str) -> io::Result<u64> {
        self.parse().map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid --{flag} integer `{self}`: {err}"),
            )
        })
    }
}

impl ParseExt for String {
    fn parse_addr(&self, flag: &str) -> io::Result<SocketAddr> {
        self.as_str().parse_addr(flag)
    }

    fn parse_u64(&self, flag: &str) -> io::Result<u64> {
        self.as_str().parse_u64(flag)
    }
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
