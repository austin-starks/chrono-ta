# Repository guidance

`CLAUDE.md` points coding agents to this file. Read it before editing.

## What this repository is

`chrono-ta` is a focused Rust library for timestamp-aware technical indicators.
The public package is `chrono-ta`; the default Rust import is `chrono_ta`.

The project is derived from Greyblake's `ta`, but it is not a drop-in fork:

- indicator constructors accept `std::time::Duration`;
- `Next` accepts `(chrono::DateTime<chrono::Utc>, value)`;
- old observations expire by timestamp;
- repeated updates in one adaptive bucket replace the current observation;
- `NextBatch` must preserve the state transition of repeated `next` calls.

Do not copy an upstream implementation without adapting and testing those
contracts.

## Public invariants

**Scalar and batch paths agree.** An optimized `next_batch` implementation must
have parity tests against a scalar loop, including continuation after the batch.
If replacement would change the recurrence, fall back to scalar processing.

**Replacement is not append.** Same-bucket input revises the current point.
Window state, derived aggregates, and the next call must behave as if the prior
version of that point had not been observed.

**Expiration is based on elapsed time.** Never replace a timestamp cutoff with a
call-count cutoff. Cover the exact boundary because some indicators deliberately
use `<` while others use `<=` according to their established semantics.

**Derived state survives serde.** Optimized caches and deques may be skipped in
serialization, but they must rebuild from authoritative state before producing
an answer. Test a deserialize followed by more updates.

**Keep claims narrower than evidence.** The crate has a focused indicator set.
Do not claim upstream parity, universal SIMD acceleration, or stable serialized
wire compatibility.

## Layout

- `src/indicators` contains stateful timestamp-aware indicators.
- `src/simd` contains slice-oriented primitives and optimized batch support.
- `src/traits.rs` defines `Next`, `NextBatch`, and `Reset`.
- `tests` contains cross-module and serde integration coverage.
- `examples` contains code that must compile as part of the release gate.

## Validation

Run these before proposing a change:

```bash
cargo fmt --check
cargo test --all-targets --all-features
cargo test --doc --all-features
cargo doc --no-deps --all-features
cargo package --list
```

For a release candidate, also build documentation with warnings denied and run
`cargo publish --dry-run` from a clean tree.

## Publishing

The crates.io package name is `chrono-ta`. The repository URL is
`https://github.com/austin-starks/chrono-ta`. Do not change either back to `ta`:
that package name belongs to the upstream project.

Publishing is a deliberate maintainer action. Update `CHANGELOG.md`, verify the
version, inspect `cargo package --list`, and run `cargo publish --dry-run` before
uploading. Never expose or commit a crates.io token.

Existing consumers may alias the package to retain old imports:

```toml
ta = { package = "chrono-ta", version = "2.1" }
```
