//! The console this shim is answering for.
//!
//! # Why the shim holds the state and the shell holds the meaning
//!
//! Principle 13: a shim holds no logic. What a setting *means*, when an event should be
//! raised and what a refusal implies all live in `orbistoun-shell`, which has no idea a
//! guest exists. This file is the other half - the statics a guest thread can reach, and
//! the accounting that says what it was not told.
//!
//! # What is not declared here, and why that is the honest answer
//!
//! `sceSystemServiceReceiveEvent` is how a title finds out it was interrupted. Its name is
//! hash-confirmed in this repository's own symbol database, so naming it costs nothing -
//! and it is **still not declared as an import**, because implementing it needs two things
//! nothing here has measured:
//!
//! - the value that means *no event is pending*, which a title checks on every poll;
//! - the layout of the structure an event is written into.
//!
//! Either could be invented and the guest would read something. That is principle 3's
//! forbidden case rather than a shortcut around it: a title told "no events" by an invented
//! constant behaves exactly like a title told the truth, right up until the constant turns
//! out to have meant something else.
//!
//! So the queue is built, the shell fills it, and what cannot be delivered is counted. The
//! declaration is a small change once those two are measured - which is a job for a probe on
//! real hardware, not for reasoning about what the numbers probably are.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use orbistoun_shell::{Delivery, EventQueue, Parameters, Settings, ShellEvent};

/// Events raised for the guest and not yet taken.
///
/// A `static` because the threads involved are not the same one and never will be: guest
/// threads drain it, and whatever carries a shell request into the worker fills it.
static QUEUE: EventQueue = EventQueue::new();

/// What the machine is set to. The shell's copy, handed over at startup.
static SETTINGS: OnceLock<Settings> = OnceLock::new();

/// Which parameter identifiers have measured answers. Empty unless configured.
static PARAMETERS: OnceLock<Parameters> = OnceLock::new();

/// Which event meanings have measured codes. Empty unless configured.
static DELIVERY: OnceLock<Delivery> = OnceLock::new();

/// How many parameter queries nothing measured could answer.
///
/// **The number that stops a placeholder reading as an answer.** `param_get_int` still
/// writes its documented zero, because an out-pointer that is never written is worse than
/// a wrong value (D171) - but a run that answered forty questions it did not understand
/// should say so, and before this it could not.
static UNANSWERED: AtomicU32 = AtomicU32::new(0);

/// Installs the console this run is answering for.
///
/// Called once, before the guest starts. Later calls are ignored rather than refused: a
/// second configure means two parts of the process disagree about who owns the settings,
/// and the guest having already read the first set makes swapping them the worse answer.
pub fn configure(settings: Settings, parameters: Parameters, delivery: Delivery) {
    let _ = SETTINGS.set(settings);
    let _ = PARAMETERS.set(parameters);
    let _ = DELIVERY.set(delivery);
}

/// What the machine is set to, or the defaults when nothing configured it.
pub fn settings() -> &'static Settings {
    SETTINGS.get_or_init(Settings::default)
}

/// Offers an event to the guest. Answers whether it could be delivered.
///
/// A `false` here is the ordinary case today and not an error: it means the shell raised
/// something real and no measured code exists to say it with.
pub fn raise(event: ShellEvent) -> bool {
    QUEUE.post(event, DELIVERY.get_or_init(Delivery::empty))
}

/// Takes the next event owed to the guest.
///
/// Nothing calls this yet - see the module note on what has to be measured before
/// `sceSystemServiceReceiveEvent` can honestly be declared.
pub fn next_event() -> Option<u32> {
    QUEUE.take()
}

/// What to tell a guest that asked for a parameter, or `None` when nothing measured knows.
///
/// The count is kept **here rather than at the call site**, so every route to an
/// unanswerable parameter is tallied by construction instead of by whoever remembers.
pub fn parameter(id: u32) -> Option<i32> {
    let answer = PARAMETERS
        .get_or_init(Parameters::empty)
        .answer(id, settings());
    if answer.is_none() {
        UNANSWERED.fetch_add(1, Ordering::Relaxed);
    }
    answer
}

/// One line for a run report: what the guest asked that nobody could answer, and what it
/// was owed and did not get.
///
/// Returns `None` when there is genuinely nothing to say, so a quiet run stays quiet.
pub fn summarise() -> Option<String> {
    let unanswered = UNANSWERED.load(Ordering::Relaxed);
    let withheld = QUEUE.withheld();
    if unanswered == 0 && withheld.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if unanswered > 0 {
        parts.push(format!(
            "{unanswered} system parameter query(s) answered with a placeholder - nothing measured says what they are"
        ));
    }
    if !withheld.is_empty() {
        parts.push(withheld.say());
    }
    Some(parts.join("; "))
}

#[cfg(test)]
mod tests {
    use orbistoun_shell::ShellEvent;

    /// **The shipped console can deliver no events, and says so rather than inventing one.**
    ///
    /// Asserted on the failure, not the success (principle 3): the interesting property is
    /// that an unmeasured event produces a report, not that a measured one works.
    #[test]
    fn an_event_with_no_measured_code_is_withheld_and_reported() {
        assert!(
            !super::raise(ShellEvent::Backgrounded),
            "nothing has measured a code, so nothing may be delivered"
        );
        assert_eq!(super::next_event(), None);

        let said = super::summarise().expect("a withheld event is worth reporting");
        assert!(said.contains("measured code"), "{said}");
    }

    /// An unanswerable parameter is counted, not silently placeheld.
    ///
    /// Shares process state with the test above by design - these are `static`s, and a test
    /// that reset them would be testing a different arrangement from the one that runs.
    #[test]
    fn a_parameter_nothing_measured_is_counted() {
        assert_eq!(super::parameter(0xdead_beef), None);

        let said = super::summarise().expect("an unanswered query is worth reporting");
        assert!(said.contains("placeholder"), "{said}");
    }
}
