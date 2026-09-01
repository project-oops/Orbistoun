//! Turning the loop without a person at the wheel.
//!
//! `docs/THE_LOOP.md` marks two steps as a person's: **read the top finding and decide what
//! it means**, and **write the code**. This crate is the first, and deliberately not the
//! second.
//!
//! # Four modules, one shape
//!
//! - [`turn`] maps a finding to the step it calls for, and runs the ones it can.
//! - [`patch`] turns what a turn measured into a change the emulator can carry.
//! - [`conformance`] grades a change against a spec rather than against a moved wall.
//! - [`experiment`] plants sentinels in a call's arguments and reads where the fault lands.
//! - [`axis`] is every other diagnostic, each rendered as the variable that carries it.
//! - [`trial`] runs the guest and reads back what happened.
//!
//! # There is no model here, by construction
//!
//! The naming loop needs one and lives in `orbistoun-propose`; this is rules and boots. That
//! is a crate boundary rather than a feature flag because a boundary cannot be forgotten -
//! the same argument `orbistoun-gpu` makes for having no dependency on `ash`, and the reason
//! a shim can run a turn without a GPU runtime linked into it (D293).
//!
//! # Nothing here writes to a tracked file
//!
//! A turn returns what it measured, and [`turn::promote`] shapes that into an entry somebody
//! else records. Deciding to change a file stays with the caller, so the decision lives in
//! one place rather than inside a sweep.

#![forbid(unsafe_code)]

pub mod axis;
pub mod conformance;
pub mod experiment;
pub mod patch;
pub mod question;
pub mod trial;
pub mod turn;

/// Why a turn could not be completed.
///
/// **One variant, and that is the finding.** The crate this split from carried five, four of
/// which were about models and grammars. A dispatcher can fail in exactly one way that is not
/// a result: the run could not be made at all. A guest that faults is a *measurement* (D293).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A run could not be made, or wrote no trace to read back.
    #[error("the run could not be read: {0}")]
    Reply(String),
}
