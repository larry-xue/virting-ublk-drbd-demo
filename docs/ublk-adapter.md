# ublk Adapter Plan

The repository does not create `/dev/ublkbN` yet. The next milestone should add
a Linux-only adapter that maps ublk requests onto the existing replication core.

## External Shape

Target command:

```bash
vrbd-ublk primary \
  --ublk-id 0 \
  --peer 10.0.0.12:9000 \
  --backing /var/lib/vrbd/vol-1.raw \
  --size-gib 40
```

Expected device:

```text
/dev/ublkb0
```

The existing CLI and tests should still work without root. ublk should be an
adapter, not the only way to exercise replication behavior.

## Candidate Rust Interface

The current candidate is the `libublk` crate:

- https://docs.rs/libublk/latest/libublk/
- https://github.com/ublk-org/ublksrv

The adapter should be feature-gated:

```toml
[features]
ublk = ["dep:libublk"]
```

## Request Mapping

```text
ublk READ(offset, len)
  -> ReplicatedDevice::read(offset, len)

ublk WRITE(offset, bytes)
  -> ReplicatedDevice::write_protocol_c(offset, bytes)

ublk FLUSH / FUA
  -> local sync + peer sync boundary
```

The hard part is not the basic read/write mapping. The hard part is preserving
flush, FUA, barrier, and failure semantics across local and remote writes.

## Acceptance Criteria for the ublk Milestone

- Creates `/dev/ublkbN` on a supported Linux host.
- `mkfs.xfs /dev/ublkbN` succeeds.
- mount + file writes succeed.
- `fio` random write/read test completes without I/O errors.
- Primary and Secondary checksums match after unmount.
- peer outage marks dirty blocks.
- peer restart + resync restores checksum equality.
- daemon crash does not leave an orphaned device without a documented cleanup
  command.

## Why qsd Still Matters for Virting

Virting already has a qsd/vhost-user-blk path for Cloud Hypervisor. The likely
integration path is:

```text
Cloud Hypervisor -> qsd -> /dev/ublkbN -> vrbd ublk daemon
```

That keeps the hypervisor-facing storage path stable while this repository
iterates on the replicated block backend.
