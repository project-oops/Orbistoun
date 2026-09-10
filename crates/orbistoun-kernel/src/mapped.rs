//! Memory a guest asked for and got.
//!
//! # The other half of a report that only prints failures
//!
//! A run says *"1 reservation(s) failed, first at 0x7400047e0000"* and nothing at all about the
//! hundreds that succeeded. That is the same asymmetry `orbistoun-fs` had before D578: the
//! failures are visible, the successes are not, and a question about *where a pointer came
//! from* can only be answered by the half that is missing.
//!
//! It is what a determinism question needs, too. Two runs of one build placed the same
//! reservation at `0x7400047e0000` and `0x740004830000` - `0x50000` apart - and the arena is
//! bump-allocated, so the difference is in what was placed *before* it. Nothing recorded what
//! that was, so the two runs could be seen to differ and not where (D581).
//!
//! # Off unless asked for
//!
//! A title maps steadily for as long as it runs, so this is gated on `ORBISTOUN_TRACE_MAPS` and
//! an ordinary run pays one atomic load per mapping - the rule `opened` follows, for the same
//! reason (principle 9).
//!
//! # Recorded here, printed by the reporting layer
//!
//! Reached from the guest's own call on the guest's own stack, so it takes a lock and pushes a
//! record and nothing else. Formatting happens after the guest has stopped (D381).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

/// One mapping the guest was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    /// Where it was placed.
    pub base: u64,
    /// How long it is, after rounding.
    pub len: u64,
    /// Whether the guest may read it, which is what decides if a dump may.
    pub readable: bool,
    /// How many guest calls had been made when it was placed.
    ///
    /// **The field that turns a diff into a question somebody can answer.** Two runs whose
    /// mapping sequences differ, compared by address alone, say only *that* they diverged;
    /// compared by call ordinal they say *when*, and the call trace says what was running.
    pub at_call: u64,
    /// Which import was most recently called when it was placed, by index.
    ///
    /// **A call ordinal says when two runs diverged; this says what was running.** Resolving an
    /// index to a name needs the import table, which this crate does not have - so the number
    /// is carried and the reporting layer, which does, spells it (D581).
    pub during: Option<u32>,
    /// Whether the guest asked for this address, rather than taking what the arena offered.
    ///
    /// **Kept because it is the difference between two kinds of address.** A hinted mapping is
    /// the guest's own arithmetic and repeats; an arena one is this process's counter and moves
    /// when anything before it moves.
    pub hinted: bool,
    /// Why the guest did not get it, when it did not.
    ///
    /// [`None`] for a mapping that was placed. **A refusal and a success are the same event
    /// from the guest's side** - it asked - and only one of them was being recorded (D602).
    pub refused: Option<String>,
}

/// Every mapping this run placed, in the order it placed them.
///
/// A list rather than a set, because **order is the finding**: a bump allocator's addresses are
/// a function of everything before them, so two runs that differ are diffed by sequence.
fn placed() -> &'static Mutex<Vec<Mapping>> {
    static PLACED: OnceLock<Mutex<Vec<Mapping>>> = OnceLock::new();
    PLACED.get_or_init(|| Mutex::new(Vec::new()))
}

/// How many mappings to remember.
///
/// A bound rather than none, for the reason `opened` gives: a list that grows with the length of
/// the run rather than with what there is to say is a leak. Generous, because the divergence
/// this exists to find can be anywhere in the sequence.
const MOST_REMEMBERED: usize = 4096;

/// Whether recording is on: unread, on, off.
static ENABLED: AtomicU8 = AtomicU8::new(UNREAD);

/// Not yet looked up.
const UNREAD: u8 = 0;
/// Looked up, and on.
const ON: u8 = 1;
/// Looked up, and off.
const OFF: u8 = 2;

/// Whether this run was asked to record what it maps.
fn enabled() -> bool {
    match ENABLED.load(Ordering::Relaxed) {
        ON => true,
        OFF => false,
        _ => {
            let on = orbistoun_env::TRACE_MAPS.is_set();
            ENABLED.store(if on { ON } else { OFF }, Ordering::Relaxed);
            on
        }
    }
}

/// Records a mapping the guest was given.
///
/// Called only where one was actually placed, so the list holds what the guest got rather than
/// what it asked for - which is the question the failure lines already answer.
pub(crate) fn note(base: u64, len: u64, readable: bool, hinted: bool) {
    if len == 0 || !enabled() {
        return;
    }
    let Ok(mut placed) = placed().lock() else {
        return;
    };
    if placed.len() >= MOST_REMEMBERED {
        return;
    }
    placed.push(Mapping {
        at_call: orbistoun_thunk::total_calls(),
        during: orbistoun_thunk::current_call(),
        base,
        len,
        readable,
        hinted,
        refused: None,
    });
}

/// Every mapping this run placed, in order.
#[must_use]
pub fn given() -> Vec<Mapping> {
    placed().lock().map(|p| p.clone()).unwrap_or_default()
}

/// Whether this run was recording.
///
/// For the reporting layer, which otherwise cannot tell an empty list from a list nobody asked
/// for - opposite findings, and the distinction `opened` already draws.
#[must_use]
pub fn recording() -> bool {
    enabled()
}

/// Records a mapping the guest asked for and did not get.
///
/// **The record kept successes only**, which is the half `wanted` teaches to keep - and it left
/// exactly one question unanswerable: a run missing a mapping could not be told from a run that
/// never attempted it. PPSA03416 alternates between those two outcomes and the difference is
/// worth one line (D602).
pub(crate) fn note_failed(base: u64, len: u64, why: &str, hinted: bool) {
    if len == 0 || !enabled() {
        return;
    }
    let Ok(mut placed) = placed().lock() else {
        return;
    };
    if placed.len() >= MOST_REMEMBERED {
        return;
    }
    placed.push(Mapping {
        at_call: orbistoun_thunk::total_calls(),
        during: orbistoun_thunk::current_call(),
        base,
        len,
        readable: false,
        hinted,
        refused: Some(why.to_owned()),
    });
}
#[cfg(test)]
mod tests {
    /// Off by default, and an ordinary run records nothing.
    #[test]
    fn nothing_is_recorded_unless_it_was_asked_for() {
        // The environment is process-wide and tests share it, so this asserts the decision
        // rather than setting the variable - mutating it here is the flaky shape D569 names.
        if orbistoun_env::TRACE_MAPS.is_set() {
            return;
        }
        super::note(0x7400_0000_0000, 0x1000, true, false);
        assert!(
            !super::given().iter().any(|m| m.base == 0x7400_0000_0000),
            "a mapping was recorded on a run that never asked for it"
        );
        assert!(!super::recording());
    }

    /// A zero-length mapping is not one.
    #[test]
    fn an_empty_mapping_is_not_recorded() {
        super::note(0x7400_0000_0000, 0, true, false);
        assert!(!super::given().iter().any(|m| m.len == 0));
    }
}
