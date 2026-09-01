//! The session this worker is running, as the shell sees it.
//!
//! # Why the worker keeps its own copy
//!
//! The window has a session too, and they are not the same object: they are in different
//! processes (D032). The window's copy decides what to draw; **this one is the copy the
//! guest is subject to**, and it is the one that must be right, because it decides whether
//! a title is told it lost the machine.
//!
//! Two copies can disagree. That is not a flaw to design away - a shim asking for something
//! this side refuses is a real event worth counting, and [`summarise`] says so at the end of
//! a run rather than letting a dropped request look like a request that did nothing.
//!
//! # Backgrounding stops threads, and says how many
//!
//! [`orbistoun_shell::Execution::Suspended`] described a behaviour without causing one for a
//! day: the state said suspended and every guest thread ran on. It now asks them to park.
//!
//! **Asks, not forces.** Threads park at the trampoline, where they hold no guest lock -
//! freezing one at an arbitrary instruction risks it holding the host heap lock, which
//! deadlocks the whole worker including whatever would have resumed it (D344).
//!
//! The cost is that a thread which stops calling imports never parks, so [`summarise`]
//! reports *how many of them stopped* rather than implying all of them did.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use orbistoun_shell::{Lifecycle, Refused, Request};

/// Where the running title stands.
static SESSION: Mutex<Lifecycle> = Mutex::new(Lifecycle::Exited);

/// Shell requests this side carried out.
static APPLIED: AtomicU32 = AtomicU32::new(0);

/// Shell requests this side refused, which means the two copies disagreed.
static REFUSED: AtomicU32 = AtomicU32::new(0);

/// Marks a title as running and in front.
///
/// Called when a run starts. Anything raised before this point has nowhere to go, which is
/// correct: there was no title to interrupt.
pub fn begin() {
    *lock() = Lifecycle::Foreground;
}

/// Marks the title as gone.
///
/// **Releases any park on the way out**, and that is not tidiness. The flag is process-wide,
/// so a run that ended while backgrounded would leave the next one's threads parking at their
/// first call - a guest that hangs for a reason belonging to the guest before it.
pub fn end() {
    *lock() = Lifecycle::Exited;
    orbistoun_core::park::release();
}

/// Where the title stands right now.
pub fn state() -> Lifecycle {
    *lock()
}

/// Carries out a shell request, raising whatever the guest is owed.
///
/// Answers what the session became, or why nothing happened.
///
/// # Errors
///
/// [`Refused`] when the request does not apply from the current state.
pub fn apply(request: Request) -> Result<Lifecycle, Refused> {
    let mut session = lock();
    match session.on(request) {
        Ok(taken) => {
            *session = taken.state;
            // Dropped before raising: `raise` reaches into the system-service statics, and
            // holding this lock across that call makes the ordering of two unrelated locks
            // part of the design for no benefit.
            let became = taken.state;
            drop(session);

            // **The state is acted on, not merely recorded.** `Execution` said `Suspended`
            // for a whole day while every guest thread kept running - a value describing a
            // behaviour rather than causing it, which is the failure this session keeps
            // finding in its own work (D344).
            //
            // Threads are *asked* to stop and park at the trampoline. How many actually did
            // is a separate question, and `summarise` reports it rather than assuming.
            match became.execution(orbistoun_shell::WhenBackgrounded::default()) {
                orbistoun_shell::Execution::Suspended => orbistoun_core::park::request(),
                orbistoun_shell::Execution::Running | orbistoun_shell::Execution::Stopped => {
                    orbistoun_core::park::release();
                }
            }
            for event in taken.raise {
                orbistoun_systemservice::console::raise(event);
            }
            APPLIED.fetch_add(1, Ordering::Relaxed);
            Ok(state())
        }
        Err(refused) => {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            Err(refused)
        }
    }
}

/// Says what the shell did to this run, on the way out.
///
/// Silent when nothing asked for anything, so an ordinary run stays as quiet as it was
/// before this existed.
pub fn summarise() {
    use std::io::Write as _;

    let applied = APPLIED.load(Ordering::Relaxed);
    let refused = REFUSED.load(Ordering::Relaxed);
    let withheld = orbistoun_systemservice::console::summarise();
    // Counted the same way events are, and for the same reason: input arriving that no
    // guest can read is a transport waiting for a measurement, and a report that said
    // nothing would leave it looking broken instead (D345).
    let input = orbistoun_input::latest::summarise();
    if applied == 0 && refused == 0 && withheld.is_none() && input.is_none() {
        return;
    }

    let mut lines = vec!["orbistoun: shell".to_owned()];
    lines.push(format!("  {applied} request(s) carried out"));
    if refused > 0 {
        // Worth a line of its own. It means the window believed the title was somewhere it
        // was not, and the guest was not told something somebody intended it to be told.
        lines.push(format!(
            "  {refused} refused - the window and the worker disagreed about where the title was"
        ));
    }
    if let Some(said) = input {
        lines.push(format!("  {said}"));
    }
    if let Some(said) = withheld {
        lines.push(format!("  {said}"));
    }
    if matches!(state(), Lifecycle::Background) {
        // **Counted rather than claimed.** Threads park cooperatively, so a guest thread in
        // a loop that calls no imports never stops - and a report saying "suspended" while
        // three of four threads ran on would be exactly the plausible output principle 3
        // forbids. The number says how much of the guest actually stopped (D344).
        let parked = orbistoun_core::park::parked();
        let threads = orbistoun_kernel::thread::all()
            .iter()
            .filter(|record| !record.finished)
            .count();
        lines.push(format!(
            "  backgrounded: {parked} of {threads} live guest thread(s) parked"
        ));
        if u32::try_from(threads).is_ok_and(|threads| parked < threads) {
            lines.push(
                "  the rest are not calling imports, so nothing can stop them where they are"
                    .to_owned(),
            );
        }
    }
    lines.push(String::new());
    let _ = std::io::stderr().write_all(lines.join("\n").as_bytes());
}

/// The guard, with a poisoned lock treated as ordinary.
///
/// A panic on one guest thread must not turn every later shell request into a panic on a
/// different one; the value behind it is a single `Copy` state with no invariant a partial
/// write could break.
fn lock() -> std::sync::MutexGuard<'static, Lifecycle> {
    SESSION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use orbistoun_shell::{Lifecycle, Request};

    /// **A shell request reaches the session and moves it, and a disagreement is counted.**
    ///
    /// The property the reader thread exists to make possible: something arriving while a
    /// run is in flight changes where the title stands.
    ///
    /// One test rather than two, because these are process-wide statics - two tests
    /// touching them run concurrently under the harness and would race on the state each
    /// one set up.
    #[test]
    fn a_request_moves_the_session_and_a_refusal_is_not_dropped() {
        super::begin();
        assert_eq!(super::state(), Lifecycle::Foreground);

        let after =
            super::apply(Request::ToShell).expect("the shell is reachable from the foreground");
        assert_eq!(after, Lifecycle::Background);

        // Asserted on the failure rather than the success: a refused request that vanished
        // would look exactly like one that was carried out and did nothing.
        super::end();
        assert!(
            super::apply(Request::Resume).is_err(),
            "there is no title to resume, and saying so is the point"
        );
    }
}
