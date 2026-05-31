# AGENTS.md

This repository is a standalone storage experiment.

- Keep the demo independent from `virting`, `bs-manager`, and `virtainer-agent`
  until the block replication model is proven.
- Do not claim production safety. Any production-oriented change needs tests for
  failure and recovery behavior.
- Prefer small, directly testable changes over broad storage abstractions.
- Run `cargo fmt --check` and `cargo test` before considering changes complete.
