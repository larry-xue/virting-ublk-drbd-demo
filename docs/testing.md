# Testing Notes

## Current Checks

```bash
cargo fmt --check
cargo test
cargo run -- demo
```

The integration tests cover:

- clean synchronous write replication,
- equal Primary and Secondary checksums,
- peer outage marking dirty blocks,
- dirty resync after the peer comes back.

## Useful Manual Fault Tests

1. Start Primary with a peer address that is not listening.
2. Issue a write and verify the client gets an error.
3. Verify `status` reports dirty blocks.
4. Start Secondary at the peer address.
5. Run `resync`.
6. Verify dirty blocks return to zero.
7. Verify Primary and Secondary checksums match.

## Tests Not Yet Implemented

- partial write failure injection,
- crash after local write but before dirty bitmap save,
- crash after remote write but before dirty bitmap clear,
- large random write workload,
- concurrent clients,
- reconnect during resync,
- request ordering under high latency,
- flush/FUA behavior through ublk.
