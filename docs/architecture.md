# Architecture

## Goal

The goal is to learn whether a small Rust implementation can cover the core of
a DRBD-like replicated block device:

- ordered writes,
- synchronous peer acknowledgement,
- persisted dirty tracking,
- reconnect and resync,
- checks that prove both replicas contain the same bytes.

It is intentionally not integrated with Virting yet. Virting integration only
makes sense after the block layer has survived direct tests.

## Current Stage 0 Data Path

```text
client CLI / tests
  -> Primary TCP server
  -> local FileBackend
  -> Secondary TCP server
  -> peer FileBackend
```

Write flow:

```text
1. client sends Write(offset, bytes) to Primary
2. Primary writes bytes to its local backing file and fsyncs data
3. Primary sends the same Write to Secondary
4. Secondary writes bytes to its backing file and fsyncs data
5. Secondary replies WriteOk
6. Primary clears the covered dirty bitmap range and replies WriteOk
```

If step 3 or 4 fails, the Primary keeps the local write, marks the affected
blocks dirty, persists the bitmap, and returns an error to the client. This is
deliberately visible because the write is not fully replicated.

## Future ublk Data Path

```text
/dev/ublkbN
  -> libublk request handler
  -> same replication core
  -> local FileBackend or block-device backend
  -> peer replication
```

The replication core should remain testable without ublk. The ublk adapter
should translate kernel block requests into the same read/write operations used
by the current CLI tests.

## Future Virting Data Path

```text
Cloud Hypervisor
  -> vhost-user-blk
  -> qemu-storage-daemon
  -> /dev/ublkbN
  -> virting-ublk-drbd daemon
```

This keeps the existing Virting qsd boundary intact. `virtainer-agent` would
eventually manage daemon lifecycle and report volume health. `bs-manager` would
eventually own volume intent, placement, operation audit, and migration checks.

## Non-Goals

- No dual-primary.
- No automatic failover.
- No quorum.
- No fencing.
- No snapshots.
- No online resize.
- No encryption.
- No production data safety claims.
- No wire compatibility with DRBD.

## Main Missing Production Semantics

- flush/FUA/barrier correctness,
- crash consistency across local and remote acknowledgement boundaries,
- split-brain prevention,
- stable volume identity and generation fencing,
- peer authentication,
- rate limiting and backpressure,
- resync checksums and sparse resync,
- full fault injection under power loss and network partitions.
