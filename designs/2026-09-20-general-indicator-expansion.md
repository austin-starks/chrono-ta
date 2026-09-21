# General indicator expansion

## Goal

Add reusable indicators from NexusTrade without importing NexusTrade's
`MarketState`, asset model, persistence macros, optimizer mutation hooks, or
strategy expression tree.

The public additions are:

- `RollingSum`
- `Lag` (`ValueAgo` is a type alias)
- `CrossAbove` and `CrossBelow`
- `TrueRange`
- `AverageTrueRange`
- `RollingVwap` and `AnchoredVwap`

## Time and replacement semantics

`RollingSum` and `Lag` follow the crate's existing adaptive replacement policy.
Their duration is the elapsed-time window and also selects the adaptive bucket
policy used by existing duration-windowed indicators.

Bar indicators need to distinguish a revised live bar from a new bar even when
they do not have a rolling window. Their constructors therefore accept an
explicit `bucket_width`. Inputs whose timestamps fall in the same fixed-width
UTC bucket replace the current bar. Inputs must arrive in nondecreasing order.

`AverageTrueRange` and `RollingVwap` accept both a window duration and a bucket
width. Expiration is based on elapsed time; observations at or before
`current_timestamp - window` are excluded.

## Formula decisions

### True range

For the first completed bucket, true range is `high - low`. Thereafter it is:

```text
max(high - low, abs(high - previous_close), abs(low - previous_close))
```

A revision to the current bucket does not change `previous_close`.

### Average true range

`AverageTrueRange` is the arithmetic mean of true-range observations retained
in the elapsed-time window. It is deliberately not Wilder's observation-count
recurrence. The distinction is documented in the public API and README.

### VWAP

VWAP uses typical price `(high + low + close) / 3` weighted by volume.

- `RollingVwap` expires contributions by elapsed time.
- `AnchoredVwap` accumulates until `Reset::reset` is called.
- A same-bucket revision removes the prior contribution before adding the
  revised contribution.
- Zero-volume buckets contribute no weight.

### Crosses

Crosses consume `(lhs, rhs)` pairs and return `bool`. A cross above occurs when
the previous bucket had `lhs <= rhs` and the current bucket has `lhs > rhs`.
Cross below is the inverse. Revising the current bucket recomputes the result
against the same previous completed bucket.

### Lag

`Lag(duration)` returns the newest value whose timestamp is at or before
`current_timestamp - duration`. It returns `None` until such a value exists.

## OHLCV input contract

`DataItem` gains public accessors and implements public `Open`, `High`, `Low`,
`Close`, and `Volume` traits. Bar indicators are generic over the traits they
need, so callers can use their own market-data structs without copying into
`DataItem`. `DataItem` is `Copy`, which also makes it compatible with the
default `NextBatch` implementation.

## Regression requirements

Every new indicator must cover:

- fixed golden vectors;
- exact elapsed-time expiration boundaries where applicable;
- same-bucket replacement;
- `Reset`;
- scalar versus `NextBatch` parity;
- continuation after a serde round trip when the `serde` feature is enabled.

The release gate remains the one in `AGENTS.md`, plus a clean-tree
`cargo publish --dry-run` before publication.
