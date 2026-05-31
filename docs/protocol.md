# Demo Protocol

The current TCP protocol is intentionally small and internal to the demo. It is
not a stable public API.

Each frame has a 40-byte big-endian header:

```text
magic[8]    = "VRBDMVP1"
version[2]  = 1
op[2]
flags[4]
offset[8]
value[8]
len[4]
reserved[4]
payload[len]
```

Current operations:

| Op | Purpose |
| --- | --- |
| `Hello` / `HelloOk` | one-frame handshake; carries block size and optional device size |
| `Write` / `WriteOk` | write payload at `offset`; `value` is payload length |
| `Read` / `ReadOk` | read `value` bytes at `offset` |
| `Status` / `StatusOk` | return human-readable daemon state |
| `Resync` / `ResyncOk` | replay dirty blocks; `FLAG_FULL_RESYNC` marks all blocks dirty first |
| `Checksum` / `ChecksumOk` | return FNV-1a checksum of the full backing file |
| `Error` | human-readable error payload |

## Acknowledgement Semantics

Primary returns `WriteOk` only after:

1. local write succeeded,
2. local data sync succeeded,
3. Secondary write succeeded,
4. Secondary data sync succeeded,
5. dirty bitmap range was cleared and persisted.

If the peer write fails, the local write remains committed, the affected blocks
are marked dirty, and the client sees `Error`.

## Why Not Use an Existing Protocol Yet

The first demo is not trying to be DRBD-compatible. It exists to keep the
failure model small enough to test:

- local write success but remote failure,
- dirty replay,
- checksum equality,
- future request translation from ublk.

If this grows beyond a demo, the protocol needs explicit version negotiation,
volume identity, generation numbers, authentication, checksums per request,
timeouts, and replay protection.
