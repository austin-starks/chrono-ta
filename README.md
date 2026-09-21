# chrono-ta

Timestamp-aware technical indicators for Rust.

[![CI](https://github.com/austin-starks/chrono-ta/actions/workflows/ci.yml/badge.svg)](https://github.com/austin-starks/chrono-ta/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue)](https://github.com/austin-starks/chrono-ta/blob/master/LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange)](https://www.rust-lang.org/)

`chrono-ta` computes moving averages, momentum, volatility, extrema, drawdown,
and drawup over elapsed-time windows. Every streaming input carries a UTC
timestamp, so a 30-day indicator means 30 calendar days of observations rather
than the last 30 calls.

The project began as a fork of [Greyblake's `ta`](https://github.com/greyblake/ta-rs),
but its input model and window semantics now differ substantially. It powers the
indicator path in [NexusTrade](https://nexustrade.io/).

## Why this exists

Observation-count windows are useful when every series has a fixed cadence. In
market systems, the same strategy may instead receive daily bars, hourly bars,
irregular historical data, or repeated live updates to the current bar.
`chrono-ta` makes time part of the indicator contract:

```text
(timestamp, value) -> indicator -> value for that point in time
```

That enables:

- windows expressed as `std::time::Duration`;
- expiration based on timestamps rather than call count;
- replacement of repeated updates within the current time bucket;
- scalar streaming and batched processing through the same stateful API;
- SIMD-backed batch paths for EMA and RSI, with scalar parity tests;
- bounded storage for long-running windowed indicators.

## `chrono-ta` versus `ta`

These crates share ancestry, not a drop-in-compatible API.

| | `chrono-ta` | Upstream `ta` |
|---|---|---|
| Window definition | Elapsed time, such as 15 minutes or 30 days | Number of observations, such as 14 values |
| Streaming input | `(DateTime<Utc>, value)` | A value or market-data item |
| Repeated live updates | Replaces the current time bucket | Every call advances state |
| Batch API | `NextBatch` plus public SIMD primitives | Scalar `Next` |
| Indicator scope | Focused set used by the timestamped engine | Broader classic indicator catalog |
| Install name | `chrono-ta` | `ta` |
| Rust import | `chrono_ta` | `ta` |

Choose upstream `ta` when you want its larger indicator catalog and
observation-count semantics. Choose `chrono-ta` when timestamps, elapsed-time
expiration, repeated current-bar updates, or batch processing are part of the
problem.

## Install

Until the first crates.io release, install from GitHub:

```toml
[dependencies]
chrono-ta = { git = "https://github.com/austin-starks/chrono-ta" }
```

Enable serialization when indicator state must survive a restart:

```toml
[dependencies]
chrono-ta = { git = "https://github.com/austin-starks/chrono-ta", features = ["serde"] }
```

After the package is published, the dependency will become:

```toml
[dependencies]
chrono-ta = "2.1"
```

## Quick start

```rust
use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use chrono_ta::indicators::ExponentialMovingAverage;
use chrono_ta::Next;
use std::time::Duration;

let mut ema = ExponentialMovingAverage::new(Duration::from_secs(3 * 60)).unwrap();
let start = Utc.with_ymd_and_hms(2026, 9, 20, 14, 30, 0).unwrap();

assert_eq!(ema.next((start, 2.0)), 2.0);
assert_eq!(
    ema.next((start + ChronoDuration::minutes(1), 5.0)),
    3.5
);
assert_eq!(
    ema.next((start + ChronoDuration::minutes(2), 1.0)),
    2.25
);
```

All indicators implement `Next<T>`. They also implement `Reset`, `Debug`,
`Display`, `Default`, and `Clone` where appropriate.

## Current-bar replacement

Streaming feeds often send several revisions of a bar before it closes. The
adaptive detector keeps those revisions from becoming several observations:

- windows shorter than five minutes use one-second buckets;
- intraday windows use one-minute buckets;
- windows of one day or longer use the library's daily-session gap rule.

Calling `next` twice inside the same bucket replaces the current observation
instead of advancing the indicator. Timestamps should therefore arrive in
nondecreasing order. This behavior is a core difference from upstream `ta`, not
an incidental optimization.

## Batch processing

`NextBatch` returns the same state transition as calling `next` repeatedly.
EMA and RSI use optimized batch implementations when no input would trigger
same-bucket replacement; other indicators use the trait's scalar fallback.

```rust
use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use chrono_ta::indicators::RelativeStrengthIndex;
use chrono_ta::NextBatch;
use std::time::Duration;

let start = Utc.with_ymd_and_hms(2026, 9, 20, 0, 0, 0).unwrap();
let inputs = vec![
    (start, 100.0),
    (start + ChronoDuration::days(1), 102.0),
    (start + ChronoDuration::days(2), 101.0),
];

let mut rsi = RelativeStrengthIndex::new(Duration::from_secs(14 * 86_400)).unwrap();
let values = rsi.next_batch(&inputs);
assert_eq!(values.len(), inputs.len());
```

The public `simd` module also exposes EMA, rate-of-change, reduction, rolling
mean, and rolling-standard-deviation primitives for callers that already own
contiguous slices.

## Indicators

| Family | Indicators |
|---|---|
| Trend | Exponential Moving Average, Simple Moving Average |
| Momentum | Relative Strength Index, Rate of Change |
| Volatility | Bollinger Bands, Standard Deviation, Mean Absolute Deviation |
| Extrema and risk | Minimum, Maximum, Max Drawdown, Max Drawup |

The narrower catalog is intentional. Indicators present in upstream `ta`, such
as MACD, stochastic oscillators, ATR, and OBV, are not currently implemented
here. Do not select this crate on the assumption that every upstream indicator
is available.

## State and serialization

The optional `serde` feature serializes indicator state. Optimized derived
state is rebuilt when needed after deserialization, and the test suite covers
continuing an indicator after a round trip.

Serialized representations are an implementation detail, not a stable wire
format. Keep the crate version with persisted state and test migrations before
upgrading a long-lived store.

## Migrating from the old repository name

GitHub redirects the former `austin-starks/ta-rs-improved` URL, so dependencies
pinned to an existing commit continue to resolve. New dependencies should use
the `chrono-ta` package and URL.

To preserve existing `use ta::...` imports while moving to a new revision,
rename the dependency locally:

```toml
[dependencies]
ta = { package = "chrono-ta", git = "https://github.com/austin-starks/chrono-ta" }
```

The source imports can then remain unchanged even though the published package
is named `chrono-ta`.

## Development

```bash
cargo fmt --check
cargo test --all-targets --all-features
cargo test --doc --all-features
cargo doc --no-deps --all-features
cargo package --list
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for defect reports, test expectations,
and pull-request scope. Security problems should be reported privately through
[SECURITY.md](SECURITY.md).

## Publishing status

`chrono-ta` is not yet published on crates.io. The exact package name is
currently unclaimed, and the manifest is prepared for a first release. A
registry release still requires a crates.io account with a verified email, an
API token, a successful clean `cargo publish --dry-run`, and the deliberate
`cargo publish` upload.

Crates.io releases are permanent and cannot be overwritten, so publishing is a
separate maintainer action from merging this repository update.

## NexusTrade

`chrono-ta` powers time-windowed technical indicators in
[NexusTrade](https://nexustrade.io/), an AI-assisted platform for researching,
testing, optimizing, and deploying systematic trading strategies.

The fork's original RSI correction is described in
[this development article](https://nexustrade.io/blog/i-used-an-ai-to-fix-a-major-bug-in-a-very-popular-open-source-technical-indicator-library-20231223).

## License and upstream credit

Released under the [MIT License](LICENSE). `chrono-ta` is derived from
[Greyblake's `ta`](https://github.com/greyblake/ta-rs), created by Sergey
Potapov and its contributors. Austin Starks maintains this timestamp-aware fork.
