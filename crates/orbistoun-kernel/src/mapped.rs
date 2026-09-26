//! Record of the mappings a guest asked for, placed or refused.
//!
//! The run report prints failed reservations; this keeps the successes too, in order, so two
//! runs whose arenas diverge can be compared by sequence. The arena is bump-allocated, so an
//! address depends on everything placed before it.
//!
//! Recording is gated on `ORBISTOUN_TRACE_MAPS`, so an ordinary run pays one atomic load per
//! mapping. A record is pushed under a lock from the guest's call and formatted by the reporting
//! layer after the guest stops.

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
    /// Compared by call ordinal, two diverging mapping sequences say when they diverged, not only
    /// that they did.
    pub at_call: u64,
    /// Which import was most recently called when it was placed, by index.
    ///
    /// This crate has no import table, so the reporting layer resolves the index to a name.
    pub during: Option<u32>,
    /// Whether the guest asked for this address, rather than taking what the arena offered.
    ///
    /// A hinted address is the guest's own arithmetic and repeats; an arena address moves when
    /// anything placed before it moves.
    pub hinted: bool,
    /// Why the guest did not get it, when it did not.
    ///
    /// [`None`] for a mapping that was placed.
    pub refused: Option<String>,
}

/// Every mapping this run placed, in the order it placed them.
///
/// A list rather than a set, because a bump allocator's addresses depend on everything before
/// them, so two runs are diffed by sequence.
fn placed() -> &'static Mutex<Vec<Mapping>> {
    static PLACED: OnceLock<Mutex<Vec<Mapping>>> = OnceLock::new();
    PLACED.get_or_init(|| Mutex::new(Vec::new()))
}

/// How many mappings to remember.
///
/// Bounded so the record does not grow with the length of the run; generous because the
/// divergence can be anywhere in the sequence.
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
/// Called only where one was placed, so the list holds what the guest got.
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
/// Lets the reporting layer tell an empty list from a list nobody asked for.
#[must_use]
pub fn recording() -> bool {
    enabled()
}

/// Records a mapping the guest asked for and did not get.
///
/// Without it, a run missing a mapping could not be told from a run that never attempted it.
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
    /// Recording is off by default, and an ordinary run records nothing.
    #[test]
    fn nothing_is_recorded_unless_it_was_asked_for() {
        // The environment is process-wide and shared by tests, so this asserts the decision rather
        // than setting the variable.
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

    /// A zero-length mapping is not recorded.
    #[test]
    fn an_empty_mapping_is_not_recorded() {
        super::note(0x7400_0000_0000, 0, true, false);
        assert!(!super::given().iter().any(|m| m.len == 0));
    }
}
