//! What the pads are doing, as the window last said.
//!
//! The window owns input so the shell's own button is seen by something other than the title;
//! the guest is in another process, so pad state travels and lands here. The window
//! sends what a title may see: the shell button stripped, and a neutral pad while the shell has
//! focus. Input crosses as a level, latest state per port (D345); `scePadReadState` reads it,
//! and [`withheld`] counts arrivals against reads.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::pad::PadState;

/// The most recent state of each port.
///
/// Latest wins rather than a queue: input is a level, and a backlog would replay presses
/// that finished seconds ago.
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
/// An absent port answers neutral: a title enumerating four pads with two configured sees two
/// quiet pads, as on a real machine with two controllers.
pub fn port(index: usize) -> PadState {
    READ.fetch_add(1, Ordering::Relaxed);
    lock().get(index).copied().unwrap_or_else(PadState::neutral)
}

/// What one port is doing, only when the window or a script has sent pad state. `None` means
/// nothing has arrived, and a read then answers the measured at-rest image (D713).
pub fn delivered(index: usize) -> Option<PadState> {
    (ARRIVED.load(Ordering::Relaxed) > 0).then(|| port(index))
}

/// How many updates arrived, and how many were read; the gap shows input nothing consumed.
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
/// A panic on one guest thread must not turn later pad reads into panics; the list holds plain
/// values with no invariant a partial write could break.
fn lock() -> std::sync::MutexGuard<'static, Vec<PadState>> {
    PORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
/// Forgets every port and every arrival, so a test can start from a machine nobody has touched.
#[cfg(test)]
pub(crate) fn forget() {
    lock().clear();
    ARRIVED.store(0, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) use tests::exclusively;

#[cfg(test)]
mod tests {
    use crate::pad::{Button, PadState};

    /// Serialises the tests that touch the port table.
    ///
    /// The table is process-global because it describes one machine's controllers, and the harness
    /// runs tests in parallel. Where the shared thing is what is under test, a lock is the fix
    /// (D324).
    pub(crate) fn exclusively() -> std::sync::MutexGuard<'static, ()> {
        static PORT_TABLE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        PORT_TABLE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// What arrives is what is read back, port by port, and the read count is kept.
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
        // An absent port is a quiet pad, not an error.
        assert_eq!(super::port(9), PadState::neutral());

        let (arrived, read) = super::withheld();
        assert!(arrived > 0 && read > 0);
    }

    /// Latest wins; there is no backlog of stale frames.
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
