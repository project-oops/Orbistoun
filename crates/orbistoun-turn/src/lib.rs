//! Turning the loop without a person at the wheel.
//!
//! A turn reads the top finding and maps it to the step it calls for, running the ones it can:
//! [`turn`] dispatches, [`trial`] runs the guest, [`experiment`] and [`axis`] are the
//! diagnostics, [`patch`] and [`conformance`] shape and grade a change, [`question`] asks what a
//! person must answer, and [`escape`] says when to stop retrying. There is no model here: the
//! naming loop lives in `orbistoun-propose`, behind a crate boundary (D293). Nothing here writes
//! a tracked file; [`turn::promote`] shapes an entry the caller records.

#![forbid(unsafe_code)]

pub mod axis;
pub mod conformance;
pub mod escape;
pub mod experiment;
pub mod patch;
pub mod question;
pub mod trial;
pub mod turn;

/// Why a turn could not be completed.
///
/// A dispatcher fails in one way that is not a result: the run could not be made at all. A
/// guest that faults is a measurement, not an error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A run could not be made, or wrote no trace to read back.
    #[error("the run could not be read: {0}")]
    Reply(String),
}
