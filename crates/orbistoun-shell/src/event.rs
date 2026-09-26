//! Lifecycle events the shell raises for the guest to drain.
//!
//! The repository has no lawful source for the vendor's event codes, so meaning and number
//! are separate: [`ShellEvent`] is our vocabulary and carries no codes, and [`Delivery`]
//! maps a meaning onto a measured code and is empty by default. An event with no code is
//! not delivered and is counted, so a run report states what the guest was owed instead of
//! a guessed code reaching it.
//!
//! The queue is bounded because a guest that never drains is common; it drops the oldest
//! event and counts the drop.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Something the shell did to the title, in our own vocabulary.
///
/// Meanings, not codes: the mapping to vendor identifiers is [`Delivery`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellEvent {
    /// The controller stopped being the title's.
    FocusLost,
    /// The controller is the title's again.
    FocusGained,
    /// The title is no longer the presenting surface.
    ///
    /// Distinct from [`Self::FocusLost`]: losing the controller is a reason to pause,
    /// losing the screen a reason to stop rendering.
    Backgrounded,
    /// The title is the presenting surface again.
    Foregrounded,
    /// The title is being asked to end.
    ///
    /// It does not promise time to comply; whether the shell waits is the shell's decision.
    Quitting,
}

impl ShellEvent {
    /// The name used in reports and in the code table.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::FocusLost => "focus-lost",
            Self::FocusGained => "focus-gained",
            Self::Backgrounded => "backgrounded",
            Self::Foregrounded => "foregrounded",
            Self::Quitting => "quitting",
        }
    }
}

/// Which guest-visible code stands for which meaning.
///
/// Empty by default. Every entry is a claim about the vendor's interface, so it comes only
/// from measurement, loaded from a runtime file so a code is added or removed without a
/// rebuild.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    /// One code per meaning. Absent means undeliverable.
    #[serde(default, rename = "code")]
    codes: BTreeMap<ShellEvent, u32>,
}

impl Delivery {
    /// A table that can deliver nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Records a measured code.
    pub fn set(&mut self, event: ShellEvent, code: u32) {
        self.codes.insert(event, code);
    }

    /// The code for a meaning, if one has been measured.
    #[must_use]
    pub fn code_for(&self, event: ShellEvent) -> Option<u32> {
        self.codes.get(&event).copied()
    }

    /// Whether anything at all can be delivered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }
}

/// What the guest was owed and did not get.
///
/// Withheld events are counted so the run report states them rather than hiding them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Withheld {
    /// How many of each meaning could not be delivered, because no code is known.
    pub unmapped: BTreeMap<ShellEvent, u32>,
    /// How many were dropped because the guest was not draining and the queue filled.
    pub overflowed: u32,
}

impl Withheld {
    /// Whether anything was withheld at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.unmapped.is_empty() && self.overflowed == 0
    }

    /// One line a person reads in a run report.
    #[must_use]
    pub fn say(&self) -> String {
        if self.is_empty() {
            return "every system event reached the guest".to_owned();
        }
        let mut parts = Vec::new();
        if !self.unmapped.is_empty() {
            let each: Vec<String> = self
                .unmapped
                .iter()
                .map(|(event, count)| format!("{} x{count}", event.label()))
                .collect();
            parts.push(format!(
                "{} withheld for want of a measured code ({})",
                self.unmapped.values().sum::<u32>(),
                each.join(", ")
            ));
        }
        if self.overflowed > 0 {
            parts.push(format!(
                "{} dropped because the guest was not draining",
                self.overflowed
            ));
        }
        parts.join("; ")
    }
}

/// Most undelivered events held before the oldest is dropped.
///
/// Small because these are lifecycle changes at the rate a person presses buttons; a queue
/// this deep already means the guest is not draining.
pub const CAPACITY: usize = 16;

/// Events the shell has raised and the guest has not yet taken.
///
/// Synchronised internally so a shim can hold one in a `static`: it is written by the
/// thread carrying shell requests into the worker and read by guest threads.
#[derive(Debug, Default)]
pub struct EventQueue {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Codes, already resolved. Only deliverable events are ever queued.
    pending: VecDeque<u32>,
    withheld: Withheld,
}

impl Withheld {
    /// An empty tally, usable in a `static`.
    const fn empty() -> Self {
        Self {
            unmapped: BTreeMap::new(),
            overflowed: 0,
        }
    }
}

impl Inner {
    const fn empty() -> Self {
        Self {
            pending: VecDeque::new(),
            withheld: Withheld::empty(),
        }
    }
}

impl EventQueue {
    /// An empty queue.
    ///
    /// `const`, because a shim holds one in a `static` shared by several threads.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::empty()),
        }
    }

    /// Offers an event to the guest.
    ///
    /// An event with no measured code is counted and not queued, so it never blocks the
    /// deliverable events behind it. Answers whether it was queued.
    pub fn post(&self, event: ShellEvent, delivery: &Delivery) -> bool {
        let mut inner = self.lock();
        let Some(code) = delivery.code_for(event) else {
            *inner.withheld.unmapped.entry(event).or_default() += 1;
            return false;
        };
        if inner.pending.len() >= CAPACITY {
            // Drop the oldest, so the most recent state survives.
            inner.pending.pop_front();
            inner.withheld.overflowed += 1;
        }
        inner.pending.push_back(code);
        true
    }

    /// Takes the next event for the guest, oldest first.
    pub fn take(&self) -> Option<u32> {
        self.lock().pending.pop_front()
    }

    /// How many are waiting.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.lock().pending.len()
    }

    /// What the guest was owed and did not get.
    #[must_use]
    pub fn withheld(&self) -> Withheld {
        self.lock().withheld.clone()
    }

    /// Empties the queue, for a title ending.
    ///
    /// The withheld tally is kept: it describes the run, not the queue, and the report is
    /// written after the title exits.
    pub fn clear(&self) {
        self.lock().pending.clear();
    }

    /// The guard, with a poisoned lock treated as ordinary.
    ///
    /// A panic in one guest thread must not panic every later caller; the queue holds plain
    /// numbers and no invariant a partial write could break.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::{CAPACITY, Delivery, EventQueue, ShellEvent, Withheld};

    /// A table that can deliver one meaning.
    fn delivering(event: ShellEvent, code: u32) -> Delivery {
        let mut delivery = Delivery::empty();
        delivery.set(event, code);
        delivery
    }

    /// The default table delivers nothing: every code must come from measurement.
    #[test]
    fn nothing_is_deliverable_until_a_code_has_been_measured() {
        assert!(Delivery::empty().is_empty());
        assert_eq!(Delivery::empty().code_for(ShellEvent::Backgrounded), None);
    }

    /// An event nobody has a code for is withheld and counted, never guessed at.
    #[test]
    fn an_unmapped_event_is_withheld_rather_than_given_a_plausible_number() {
        let queue = EventQueue::new();
        let nothing = Delivery::empty();

        assert!(!queue.post(ShellEvent::Backgrounded, &nothing));
        assert_eq!(
            queue.waiting(),
            0,
            "the guest must not receive an invented code"
        );
        assert_eq!(queue.take(), None);

        let withheld = queue.withheld();
        assert_eq!(withheld.unmapped.get(&ShellEvent::Backgrounded), Some(&1));
        assert!(
            withheld.say().contains("measured code"),
            "the report must say why: {}",
            withheld.say()
        );
    }

    /// A measured code reaches the guest, in the order it happened.
    #[test]
    fn a_measured_code_is_delivered_oldest_first() {
        let mut delivery = delivering(ShellEvent::FocusLost, 7);
        delivery.set(ShellEvent::Backgrounded, 9);
        let queue = EventQueue::new();

        assert!(queue.post(ShellEvent::FocusLost, &delivery));
        assert!(queue.post(ShellEvent::Backgrounded, &delivery));

        assert_eq!(queue.take(), Some(7));
        assert_eq!(queue.take(), Some(9));
        assert_eq!(queue.take(), None);
        assert!(queue.withheld().is_empty());
    }

    /// A partly-measured table delivers what it knows and does not block on the rest.
    #[test]
    fn an_unmapped_event_does_not_block_the_ones_that_work() {
        let delivery = delivering(ShellEvent::Foregrounded, 4);
        let queue = EventQueue::new();

        queue.post(ShellEvent::Backgrounded, &delivery);
        queue.post(ShellEvent::Foregrounded, &delivery);

        assert_eq!(queue.take(), Some(4), "the known one still arrives");
        assert_eq!(queue.withheld().unmapped.len(), 1);
    }

    /// A guest that never drains does not grow the queue without bound; drops are counted.
    #[test]
    fn a_guest_that_never_drains_overflows_visibly_rather_than_leaking() {
        let delivery = delivering(ShellEvent::FocusLost, 1);
        let queue = EventQueue::new();

        for _ in 0..CAPACITY + 3 {
            queue.post(ShellEvent::FocusLost, &delivery);
        }

        assert_eq!(queue.waiting(), CAPACITY);
        assert_eq!(queue.withheld().overflowed, 3);
        assert!(
            queue.withheld().say().contains("not draining"),
            "{}",
            queue.withheld().say()
        );
    }

    /// A quiet run says so, rather than saying nothing.
    #[test]
    fn a_run_that_withheld_nothing_reports_that_plainly() {
        assert!(Withheld::default().is_empty());
        assert_eq!(
            Withheld::default().say(),
            "every system event reached the guest"
        );
    }

    /// Clearing drops what is queued and keeps what the run is owed an account of.
    #[test]
    fn ending_a_title_clears_the_queue_but_not_the_account_of_what_it_missed() {
        let queue = EventQueue::new();
        queue.post(ShellEvent::Quitting, &Delivery::empty());
        queue.post(ShellEvent::FocusLost, &delivering(ShellEvent::FocusLost, 2));

        queue.clear();

        assert_eq!(queue.waiting(), 0);
        assert!(
            !queue.withheld().is_empty(),
            "the report describes the run, and is written after it ends"
        );
    }
}
