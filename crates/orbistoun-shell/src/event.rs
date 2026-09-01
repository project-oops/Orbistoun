//! Telling the guest that something happened to it.
//!
//! # The provenance problem, and why this file is shaped around it
//!
//! A title finds out it was interrupted by draining an event queue the system fills. So
//! the shell has to put something in that queue - and **this repository has no lawful
//! source for what the vendor's events are numbered**.
//!
//! There are three ways to handle that and two of them are the same mistake. Inventing a
//! plausible code is principle 3's forbidden case exactly: the guest reads a number that
//! means something specific to it, acts on it, and the failure surfaces somewhere else
//! entirely. Picking zero is the same thing wearing a humbler hat.
//!
//! The third is to **separate the meaning from the number**. [`ShellEvent`] is our
//! vocabulary and carries no codes at all; [`Delivery`] maps a meaning onto a code and is
//! **empty until something measures one** (principle 5). An event with no code is not
//! delivered, and what is not delivered is *counted* - so a run report can say "the guest
//! was owed four events and got none, because no code is known for them" rather than
//! quietly behaving as though the shell were working.
//!
//! That is a worse emulator today and the only version that can become a correct one. The
//! codes arrive the way every other fact here does: measured, attributed, and checkable
//! (`known_by`), not recalled.
//!
//! # Why the queue is bounded
//!
//! A guest that never drains is the ordinary case, not the exception - most titles here do
//! not reach their event loop. An unbounded queue behind a guest that never reads is a slow
//! leak that looks like nothing at all, so the queue has a ceiling and says what it dropped.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Something the shell did to the title, in our own vocabulary.
///
/// **Meanings, not codes.** Nothing here is a vendor identifier and nothing here may
/// become one; the mapping is [`Delivery`], kept separate precisely so this enum stays
/// something the repository is entitled to assert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellEvent {
    /// The controller stopped being the title's.
    FocusLost,
    /// The controller is the title's again.
    FocusGained,
    /// The title is no longer the presenting surface.
    ///
    /// Distinct from [`Self::FocusLost`], and a title may reasonably act on one and not
    /// the other: losing the pad is a reason to pause, losing the screen is a reason to
    /// stop rendering.
    Backgrounded,
    /// The title is the presenting surface again.
    Foregrounded,
    /// The title is being asked to end.
    ///
    /// **Not a guarantee of time to comply.** Whether the shell waits for the guest to
    /// finish is the shell's decision, and a title that treats this as a promise of a
    /// clean shutdown window is making an assumption nothing here has made to it.
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
/// **Empty by default and that is the honest state.** Every entry is a claim about the
/// vendor's interface, so one arrives only by measurement - and the file it is loaded from
/// is a runtime input rather than a compiled constant, which is what lets a code be added
/// without a rebuild and removed when it turns out to be wrong (principle 5).
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
/// **The point of the whole arrangement.** Withholding an event is defensible; withholding
/// it silently is the failure this project keeps writing decisions about. A count that
/// reaches a run report turns "the shell does nothing" from a mystery into a measurement.
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
/// Small on purpose. These are lifecycle changes at the rate a person presses buttons, so
/// a queue this deep already means the guest has not looked in a very long time - and a
/// larger number would only postpone the same report.
pub const CAPACITY: usize = 16;

/// Events the shell has raised and the guest has not yet taken.
///
/// Synchronised internally so a shim can hold one in a `static`: it is written by whatever
/// carries shell requests into the worker and read by guest threads, which are not the
/// same thread and never will be.
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
    /// `const`, because a shim holds one in a `static`: the guest threads that drain it and
    /// whatever carries shell requests into the worker are different threads, so there is no
    /// single owner to hand it to.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::empty()),
        }
    }

    /// Offers an event to the guest.
    ///
    /// **The undeliverable case is decided here rather than at the far end**, so the queue
    /// only ever holds things the guest can actually be given. An event with no measured
    /// code that sat in the queue waiting for one would block every later event behind it,
    /// and the guest would be denied the events that *do* work because of the ones that do
    /// not.
    ///
    /// Answers whether it was queued.
    pub fn post(&self, event: ShellEvent, delivery: &Delivery) -> bool {
        let mut inner = self.lock();
        let Some(code) = delivery.code_for(event) else {
            *inner.withheld.unmapped.entry(event).or_default() += 1;
            return false;
        };
        if inner.pending.len() >= CAPACITY {
            // Oldest, so the most recent picture of what happened survives. A title that
            // finally drains cares far more about "you are backgrounded now" than about
            // the focus change fifteen presses ago.
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
    /// The withheld tally is **kept**: it describes the run, not the queue, and a report
    /// written after the title exited is exactly when somebody reads it.
    pub fn clear(&self) {
        self.lock().pending.clear();
    }

    /// The guard, with a poisoned lock treated as ordinary.
    ///
    /// A panic in one guest thread must not turn every later event into a panic in a
    /// different one; the queue holds plain numbers and no invariant a partial write could
    /// break.
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

    /// **The default state of this crate is "cannot deliver anything", and it is deliberate.**
    ///
    /// If this test ever needs changing because codes were added to the shipped default,
    /// the question to ask first is where those codes were measured.
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

    /// **A partly-measured table delivers what it knows and does not block on what it does not.**
    ///
    /// The failure this prevents: one unmapped meaning parked at the head of the queue,
    /// denying the guest every deliverable event behind it.
    #[test]
    fn an_unmapped_event_does_not_block_the_ones_that_work() {
        let delivery = delivering(ShellEvent::Foregrounded, 4);
        let queue = EventQueue::new();

        queue.post(ShellEvent::Backgrounded, &delivery);
        queue.post(ShellEvent::Foregrounded, &delivery);

        assert_eq!(queue.take(), Some(4), "the known one still arrives");
        assert_eq!(queue.withheld().unmapped.len(), 1);
    }

    /// A guest that never drains does not grow the queue without bound, and it is said so.
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
