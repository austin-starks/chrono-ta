# Contributing

Issues and focused pull requests are welcome. Start with a specific input
sequence, timestamp, indicator, and result that is wrong or unexpectedly slow.

## Report a defect

Use the repository's bug-report form and include:

- the `chrono-ta` version or exact Git commit;
- the smallest timestamped input sequence that reproduces the problem;
- the expected and actual outputs;
- whether the calls used `next` or `next_batch`;
- enabled features, Rust version, operating system, and architecture.

Do not include brokerage credentials, market-data credentials, proprietary
datasets, or non-public strategy data. Report security problems through
[SECURITY.md](SECURITY.md), not a public issue.

## Set up the repository

Install stable Rust, then run:

```bash
cargo test --all-targets --all-features
cargo run --example time_window
```

Before opening a pull request, run:

```bash
cargo fmt --check
cargo test --all-targets --all-features
cargo test --doc --all-features
cargo doc --no-deps --all-features
cargo package --list
```

## Test the time semantics

An indicator change should cover the failure mode, not only the happy path.
Depending on the code, include:

- the first observation and a partially filled window;
- the exact expiration boundary;
- a timestamp after one or more observations expire;
- two updates in the same adaptive bucket;
- reset and then reuse;
- scalar versus batch parity;
- serialization followed by continued updates when `serde` state changes.

Use deterministic inputs. Performance changes need a correctness oracle before
an optimized implementation and should report the workload that improved.

## Add an indicator

Open an issue before implementing a large new indicator family. Explain why it
belongs in a duration-windowed library, its timestamp and current-bar
replacement semantics, its output type, and the reference formula. Upstream
`ta` code cannot be copied unchanged because its observation-count contract is
different.

## Pull-request scope

Keep changes narrow. Do not combine a new indicator, dependency upgrades, and
unrelated cleanup. A useful pull request says what previously failed, adds a
regression test, and records exactly which checks ran.

Automated contributors must read [AGENTS.md](AGENTS.md). The same evidence and
scope rules apply regardless of who authored the patch.
