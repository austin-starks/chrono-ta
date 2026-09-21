//! Timestamp-aware, duration-windowed technical indicators.
//!
//! Unlike observation-count libraries, `chrono-ta` accepts
//! `(chrono::DateTime<chrono::Utc>, value)` inputs and expires observations by
//! elapsed time. Start with [`indicators`] and the [`Next`] trait for streaming
//! updates, or [`NextBatch`] for an equivalent batched state transition.

#[cfg(test)]
#[macro_use]
mod test_helper;

#[cfg(test)]
mod helpers;

pub mod errors;
pub mod indicators;
pub mod simd;

mod traits;
pub use crate::traits::*;

mod data_item;
pub use crate::data_item::DataItem;
