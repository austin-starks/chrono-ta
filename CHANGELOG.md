# Changelog

## 2.1.0 - 2026-09-20

- Renamed the project and Cargo package from `ta-rs-improved`/`ta` to
  `chrono-ta`/`chrono_ta` so the package has a distinct identity and can be
  published without colliding with upstream `ta`.
- Added `NextBatch`, SIMD-backed EMA and RSI batches, and public slice-oriented
  SIMD primitives.
- Made rolling extrema, maximum drawdown, and maximum drawup amortized O(1) per
  update while preserving scalar reference tests.
- Documented elapsed-time windows, same-bucket replacement, upstream
  differences, migration, and crates.io release status.
- Added a source-controlled animation that demonstrates the semantic difference
  between observation-count and elapsed-time windows.
- Added public contribution, security, conduct, CI, and release guidance.

## Earlier upstream history

* Add Weighted Moving Average (WMA)


#### v0.5.0 - 2021-06-27

* [breaking] - get rid of error-chain. ta::Error -> ta::TaError
* Implement Mean Absolute Deviation (MAD)
* Implement Commodity Channel Index (CCI)
* More efficient MoneyFlowIndex
* Fix identical TypicalPrice bug in MoneyFlowIndex

#### v0.4.0 - 2020-11-03

* [breaking] Unify parameters for the indicators
* Implement Chandelier Exit (CE)

#### v0.3.1 - 2020-10-20

* Fix NaN bug in StandardDeviation

#### v0.3.0 - 2020-10-06

* Implement Percentage Price Oscillator (PPO)
* More efficient BollingerBands
* More efficient FastStochastic
* More efficient SlowStochastic
* More efficient StandardDeviation
* More efficient Minimum
* More efficient Maximum
* More efficient SimpleMovingAverage
* Serde support

#### v0.2.0 - 2020-08-31

* Breaking: MovingAverageConvergenceDivergence now returns MovingAverageConvergenceDivergenceOutput instead of tuple
* Implement Keltner Channel (KC)
* Update error-chain dependency: 0.11 -> 0.12

#### v0.1.5 - 2019-12-16

* StandardDeviation Implementation
* More Efficient BollingerBands

#### v0.1.4 - 2019-04-09

* Implement On Balance Volume (OBV)

#### v0.1.3 - 2019-03-28

* Implement Money Flow Index (MFI)
* Add benchmarks

#### v0.1.2 - 2019-03-17

* Implement Bollinger Bands (BB)

#### v0.1.1 - 2019-02-26

* Implement Kaufman's Efficiency Ratio (ER)
* Implement Rate of Change (ROC)
* Migrate to Rust 2018 edition

#### v0.1.0 - 2017-12-05

* Initial release
* Implemented indicators
  * Trend
    * Exponential Moving Average (EMA)
    * Simple Moving Average (SMA)
  * Oscillators
    * Relative Strength Index (RSI)
    * Fast Stochastic
    * Slow Stochastic
    * Moving Average Convergence Divergence (MACD)
  * Other
    * Minimum
    * Maximum
    * True Range
    * Average True Range (AR)
    * Rate of Change (ROC)
