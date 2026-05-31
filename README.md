# virting-ublk-drbd-demo

Standalone Rust MVP for a DRBD-like replicated block device intended to grow
into a Linux `ublk` target.

This repository is deliberately separate from `virting`, `bs-manager`, and
`virtainer-agent`. The first milestone isolates the risky part: block-device
replication semantics. It does not depend on the Virting manager or agent, and
it does not create `/dev/ublkbN` yet.

## Status

Stage 0 is implemented:

- Primary and Secondary TCP daemons.
- Fixed-size file-backed block device model.
- Protocol-C-like writes: Primary commits locally, sends the same write to the
  Secondary, and only returns success after the peer acknowledges.
- Dirty bitmap persisted next to the Primary backing file.
- Resync of dirty blocks after a peer outage.
- CLI for write/read/status/checksum/resync.
- Integration tests for clean replication and dirty replay.

This is not production DRBD, not wire-compatible with DRBD, and not safe for
real VM data. It is a lab prototype for finding the hard parts before tying it
to Cloud Hypervisor, qsd, or Virting control-plane state.

## Why This Exists

Virting's current Lite path uses per-VM `qcow2` files on one host. That is good
for the single-host MVP, but it blocks real cluster workflows:

- A host reboot interrupts all local VMs.
- Live migration needs the destination host to see the same block data.
- A single local `qcow2` file does not survive host failure as a cluster storage
  primitive.

The long-term product path is still:

```text
Cloud Hypervisor
  -> vhost-user-blk
  -> qemu-storage-daemon
  -> /dev/ublkbN
  -> Rust ublk replicated block target
  -> local backing file/device + peer replication
```

Stage 0 stops before `ublk` so the replication model can be tested without root
permissions, kernel module setup, or VM boot complexity.

## Quick Start

Run the in-process local demo:

```bash
cargo run -- demo
```

Expected output includes Primary and Secondary addresses, matching reads, and
matching checksums.

Run tests:

```bash
cargo test
```

## Manual Two-Daemon Demo

Terminal 1:

```bash
cargo run -- secondary \
  --listen 127.0.0.1:7001 \
  --backing /tmp/vrbd-secondary.img \
  --size-mib 64
```

Terminal 2:

```bash
cargo run -- primary \
  --listen 127.0.0.1:7000 \
  --peer 127.0.0.1:7001 \
  --backing /tmp/vrbd-primary.img \
  --bitmap /tmp/vrbd-primary.dirty \
  --size-mib 64
```

Terminal 3:

```bash
cargo run -- write --target 127.0.0.1:7000 --offset 0 --data "hello vrbd"
cargo run -- read --target 127.0.0.1:7000 --offset 0 --len 10
cargo run -- read --target 127.0.0.1:7001 --offset 0 --len 10
cargo run -- checksum --target 127.0.0.1:7000
cargo run -- checksum --target 127.0.0.1:7001
cargo run -- status --target 127.0.0.1:7000
```

## Dirty Resync Demo

Start Primary first with a peer address that is not listening:

```bash
cargo run -- primary \
  --listen 127.0.0.1:7100 \
  --peer 127.0.0.1:7101 \
  --backing /tmp/vrbd-primary-outage.img \
  --bitmap /tmp/vrbd-primary-outage.dirty \
  --size-mib 64
```

This write commits locally but returns an error because the peer did not
acknowledge:

```bash
cargo run -- write --target 127.0.0.1:7100 --offset 4096 --data "peer was down"
cargo run -- status --target 127.0.0.1:7100
```

Then start Secondary and replay dirty blocks:

```bash
cargo run -- secondary \
  --listen 127.0.0.1:7101 \
  --backing /tmp/vrbd-secondary-outage.img \
  --size-mib 64

cargo run -- resync --target 127.0.0.1:7100
cargo run -- read --target 127.0.0.1:7101 --offset 4096 --len 13
```

## Repository Layout

```text
src/backend.rs   file-backed block backend and checksum
src/bitmap.rs    persisted dirty bitmap
src/protocol.rs  small binary TCP frame protocol
src/server.rs    Primary and Secondary daemon logic
src/client.rs    client helpers used by CLI and tests
src/cli.rs       dependency-free command parser
tests/           end-to-end replication tests
docs/            architecture, protocol, and ublk integration notes
```

## References

- Linux ublk documentation: https://kernel.org/doc/html/latest/block/ublk.html
- ublksrv userspace server: https://github.com/ublk-org/ublksrv
- Rust `libublk` crate docs: https://docs.rs/libublk/latest/libublk/
- LINBIT DRBD overview: https://linbit.com/drbd/

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license
