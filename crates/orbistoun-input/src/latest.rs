//! What the pads are doing, as the window last said.
//!
//! # Why this is here and not filled in
//!
//! The window owns input, because the shell's own button has to be seen by something that
//! is not the title (D326). The guest is in another process, so pad state has to travel -
//! and this is where it lands.
//!
//! **Nothing reads it yet, and that is not an oversight.** `scePadReadStateExt` writes a
//! structure whose size and layout nobody here has measured, so the shim that would consume
//! this is deliberately unimplemented. Building the transport anyway is the same call the
//! event queue makes: the mechanism is ours and testable, the payload's *encoding* is a
//! measurement, and mixing the two is what produces confident wrong answers (D345).
//!
//! What that buys now is not nothing. The window sends what a **title is allowed to see** -
//! the shell's own button stripped, and a neutral pad while the shell has focus - so the
//! arbitration that was previously a tested function with no observable effect now has one,
//! and [`withheld`] counts what arrived against what was consumed.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::pad::PadState;

/// The most recent state of each port.
///
/// **Latest wins rather than a queue.** Input is a level, not a stream of events: a title
/// asks what the pad is doing *now*, and a backlog of stale frames is worse than none -
/// it would replay presses that finished seconds ago.
static PORTS: Mutex<Vec<PadState>> = Mutex::new(Vec::new());

/// How many updates have arrived.
static ARRIVED: AtomicU32 = AtomicU32::new(0);

/// How many times a guest has actually read one.
static READ: AtomicU32 = AtomicU32::new(0);

/// Records what the window says the pads are doing.
pub fn arrived(pads: &[PadState]) {
    let mut held = lock();
    held.clear();
    held.extend_from_slice(pads);
    ARRIVED.fetch_add(1, Ordering::Relaxed);
}

/// What one port is doing, or a pad nobody is holding when there is no such port.
///
/// **An absent port answers neutral rather than nothing.** A title that enumerates four pads
/// and finds two configured should see two quiet pads, not an error - that is the state of a
/// real machine with two controllers plugged in.
pub fn port(index: usize) -> PadState {
    READ.fetch_add(1, Ordering::Relaxed);
    lock().get(index).copied().unwrap_or_else(PadState::neutral)
}

/// How many updates arrived, and how many were read.
///
/// **The gap is the point.** Until a layout is measured the read count stays at zero however
/// much input arrives, and a report saying so is the difference between a transport that is
/// waiting for something and one that is quietly broken.
#[must_use]
pub fn withheld() -> (u32, u32) {
    (
        ARRIVED.load(Ordering::Relaxed),
        READ.load(Ordering::Relaxed),
    )
}

/// One line for a run report, or nothing when no input ever arrived.
#[must_use]
pub fn summarise() -> Option<String> {
    let (arrived, read) = withheld();
    if arrived == 0 {
        return None;
    }
    if read == 0 {
        return Some(format!(
            "{arrived} pad update(s) arrived and none reached the guest - no measured layout to write one into"
        ));
    }
    Some(format!("{arrived} pad update(s) arrived, {read} read"))
}

/// The guard, with a poisoned lock treated as ordinary.
///
/// A panic on one guest thread must not turn every later pad read into a panic on another;
/// what is behind it is a list of plain values with no invariant a partial write could break.
fn lock() -> std::sync::MutexGuard<'static, Vec<PadState>> {
    PORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use crate::pad::{Button, PadState};

    /// Serialises the tests that touch the port table.
    ///
    /// **The table is process-global, because it describes one machine's controllers**, and
    /// the harness runs tests in parallel. Without this, a test publishing one pad truncates
    /// the table another test is asserting two ports of, and which one fails depends on
    /// timing - a gate that fails once a day is a gate people stop reading.
    ///
    /// Fifth appearance of this hazard, after `orbistoun-abi`'s shared array, D323's fixed
    /// addresses, the `.bss` fill cache and the format-fault counter. Where the shared thing
    /// *is* what is under test, a lock is the fix; where it is not, passing it is (D372).
    fn exclusively() -> std::sync::MutexGuard<'static, ()> {
        static PORT_TABLE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        PORT_TABLE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// **What arrives is what can be read back, port by port.**
    ///
    /// The one property the transport has to have, and the only one testable before a layout
    /// exists to hand it to a guest.
    #[test]
    fn what_arrives_is_what_is_read_back_and_the_gap_is_counted() {
        let _guard = exclusively();
        let mut first = PadState::neutral();
        first.set(Button::South, true);
        let mut second = PadState::neutral();
        second.set(Button::Start, true);

        super::arrived(&[first, second]);

        assert!(super::port(0).is_down(Button::South));
        assert!(super::port(1).is_down(Button::Start));
        // **An absent port is a quiet pad, not an error.** A title enumerating four while two
        // are configured should find two nobody is holding.
        assert_eq!(super::port(9), PadState::neutral());

        let (arrived, read) = super::withheld();
        assert!(arrived > 0 && read > 0);
    }

    /// **Latest wins; there is no backlog of stale frames.**
    ///
    /// Input is a level rather than a stream. A queue would replay presses that finished
    /// seconds ago, which is worse than losing them.
    #[test]
    fn a_later_update_replaces_an_earlier_one() {
        let _guard = exclusively();
        let mut pressed = PadState::neutral();
        pressed.set(Button::North, true);
        super::arrived(&[pressed]);
        super::arrived(&[PadState::neutral()]);

        assert!(
            !super::port(0).is_down(Button::North),
            "the release is what a title should see, not the press before it"
        );
    }
}
