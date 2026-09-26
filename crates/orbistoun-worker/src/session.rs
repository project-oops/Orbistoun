//! The session this worker is running, as the shell sees it.
//!
//! The window keeps its own session in another process (D032) and uses it to decide what to
//! draw; this copy is the one the guest is subject to. The two can disagree, and a request this
//! side refuses is counted and reported by [`summarise`].
//!
//! Backgrounding asks guest threads to park at the trampoline, where they hold no guest lock
//! (D344). A thread that stops calling imports never parks, so [`summarise`] reports how many
//! did.

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
/// Called when a run starts; before it there is no title to interrupt.
pub fn begin() {
    *lock() = Lifecycle::Foreground;
}

/// Marks the title as gone.
///
/// Releases any park: the flag is process-wide, and a run that ended while backgrounded would
/// otherwise park the next run's threads at their first call.
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
            // Dropped before raising, so this lock is never ordered against the system-service
            // statics `raise` takes.
            let became = taken.state;
            drop(session);

            // The state is acted on: threads are asked to park at the trampoline (D344), and
            // `summarise` reports how many did.
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
/// Silent when nothing asked for anything.
pub fn summarise() {
    let applied = APPLIED.load(Ordering::Relaxed);
    let refused = REFUSED.load(Ordering::Relaxed);
    let withheld = orbistoun_systemservice::console::summarise();
    // Input that no guest read is counted too, so a silent report never hides it.
    let input = orbistoun_input::latest::summarise();
    if applied == 0 && refused == 0 && withheld.is_none() && input.is_none() {
        return;
    }

    let mut lines = vec!["shell".to_owned()];
    lines.push(format!("  {applied} request(s) carried out"));
    if refused > 0 {
        // The window believed the title was somewhere it was not, so the guest missed an event.
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
        // Counted rather than claimed: threads park cooperatively, and one that calls no
        // imports never stops (D344).
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
    tracing::info!("{}", lines.join("\n"));
}

/// The guard, with a poisoned lock treated as ordinary.
///
/// A panic on one guest thread must not turn later shell requests into panics; the value is a
/// single `Copy` state no partial write can break.
fn lock() -> std::sync::MutexGuard<'static, Lifecycle> {
    SESSION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use orbistoun_shell::{Lifecycle, Request};

    /// A shell request moves the session, and a refused request is an error, not dropped. One
    /// test, because the state is process-wide and two tests would race on it.
    #[test]
    fn a_request_moves_the_session_and_a_refusal_is_not_dropped() {
        super::begin();
        assert_eq!(super::state(), Lifecycle::Foreground);

        let after =
            super::apply(Request::ToShell).expect("the shell is reachable from the foreground");
        assert_eq!(after, Lifecycle::Background);

        super::end();
        assert!(
            super::apply(Request::Resume).is_err(),
            "there is no title to resume, and saying so is the point"
        );
    }
}
