//! The machine settings and event queue the system-service shim answers from.
//!
//! The shim holds state, not meaning: what a setting means and when an event is raised live in
//! `orbistoun-shell`. This file holds the statics a guest thread can reach and counts what the
//! guest could not be told.
//!
//! `sceSystemServiceReceiveEvent` is not declared as an import. Implementing it needs the value
//! meaning no event is pending and the layout of the event structure, and neither is measured; an
//! invented constant would read as the truth (D010). Events that cannot be delivered are counted.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use orbistoun_shell::{Delivery, EventQueue, Parameters, Settings, ShellEvent};

/// Events raised for the guest and not yet taken.
///
/// A `static` because guest threads drain it and the thread carrying shell requests fills it.
static QUEUE: EventQueue = EventQueue::new();

/// What the machine is set to. The shell's copy, handed over at startup.
static SETTINGS: OnceLock<Settings> = OnceLock::new();

/// Which parameter identifiers have measured answers. Empty unless configured.
static PARAMETERS: OnceLock<Parameters> = OnceLock::new();

/// Which event meanings have measured codes. Empty unless configured.
static DELIVERY: OnceLock<Delivery> = OnceLock::new();

/// How many parameter queries nothing measured could answer.
///
/// `param_get_int` still writes its placeholder zero (D171); this count lets a run report how
/// many answers were placeholders.
static UNANSWERED: AtomicU32 = AtomicU32::new(0);

/// Installs the console this run is answering for.
///
/// Called once, before the guest starts. Later calls are ignored: the guest may already have
/// read the first settings, so swapping them is the worse answer.
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
/// `false` is not an error: the shell raised something real and no measured code exists for it.
pub fn raise(event: ShellEvent) -> bool {
    QUEUE.post(event, DELIVERY.get_or_init(Delivery::empty))
}

/// Takes the next event owed to the guest.
///
/// Unused until `sceSystemServiceReceiveEvent` is declared (see the module note).
pub fn next_event() -> Option<u32> {
    QUEUE.take()
}

/// What to tell a guest that asked for a parameter, or `None` when nothing measured knows.
///
/// The count is kept here so every route to an unanswerable parameter is tallied.
pub fn parameter(id: u32) -> Option<i32> {
    let answer = PARAMETERS
        .get_or_init(Parameters::empty)
        .answer(id, settings());
    if answer.is_none() {
        UNANSWERED.fetch_add(1, Ordering::Relaxed);
    }
    answer
}

/// One line for a run report: what the guest asked that nobody could answer, and what it was
/// owed and did not get. `None` when there is nothing to say.
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

    /// An unmeasured event produces a report instead of an invented delivery.
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

    /// An unanswerable parameter is counted.
    ///
    /// Shares the `static` state with the test above, as a run does.
    #[test]
    fn a_parameter_nothing_measured_is_counted() {
        assert_eq!(super::parameter(0xdead_beef), None);

        let said = super::summarise().expect("an unanswered query is worth reporting");
        assert!(said.contains("placeholder"), "{said}");
    }
}
