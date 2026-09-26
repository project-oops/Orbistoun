//! The escape hatch: when an autonomous trial loop should stop and ask a person.
//!
//! Most of the try-measure-verdict cycle in `docs/THE_LOOP.md` is progress a loop can make on its
//! own. Four situations are not, and grinding on them wastes boots and risks a stub that compiles
//! and reads like progress:
//!
//! - an architectural wall the loop cannot pass without new capability: an unknown command-buffer
//!   opcode, an untranslatable shader instruction, or an ABI violation;
//! - a spin deadlock: the guest stuck in its own synchronisation, calling the host nothing (the
//!   `quiet` signal);
//! - a regression: a change that made the guest reach less than doing nothing did (D129);
//! - retry exhaustion: [`RETRY_LIMIT`] attempts on one finding with no further between them.
//!
//! Deciding which trigger tripped is a pure function of the attempts observed. On a trip the hatch
//! returns an [`Escalation`] for a person and the inert trial patches are discarded; it writes no
//! file and never touches the hardware.

use crate::patch::Patch;

/// How many attempts on one finding without further before the loop gives up on it.
pub const RETRY_LIMIT: u32 = 3;

/// The kind of architectural wall, so an escalation names the capability that is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wall {
    /// A GPU command-buffer opcode the packet walker does not recognise.
    UnknownGpuOpcode,
    /// A shader instruction the shader recompiler cannot translate.
    UntranslatableInstruction,
    /// The guest crossed the ABI boundary in a way the calling convention forbids.
    AbiViolation,
}

impl Wall {
    /// How the escalation names the missing capability.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::UnknownGpuOpcode => "an unrecognised GPU packet opcode",
            Self::UntranslatableInstruction => "an untranslatable shader instruction",
            Self::AbiViolation => "an ABI boundary violation",
        }
    }
}

/// Why an autonomous loop stopped and asked a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// A wall the loop cannot pass without new capability, and which kind.
    ArchitecturalWall(Wall),
    /// The guest is stuck in its own synchronisation, calling the host nothing and reaching
    /// nowhere.
    SpinDeadlock,
    /// A change made the guest reach less than the baseline: verdict BACK.
    Regression,
    /// [`RETRY_LIMIT`] attempts on one finding, none of them further.
    RetryExhaustion,
}

impl Trigger {
    /// How the escalation names the trigger.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::ArchitecturalWall(wall) => format!("architectural wall ({})", wall.label()),
            Self::SpinDeadlock => "spin deadlock".to_owned(),
            Self::Regression => "regression".to_owned(),
            Self::RetryExhaustion => "retry exhaustion".to_owned(),
        }
    }
}

/// One attempt's signals, as far as the hatch reads them.
///
/// Built from a run's trace by the loop that drives the hatch (`reached` from the distinct imports,
/// `spinning` from the trace's `quiet` signal, `wall` from an ABI report, an unwalkable submission
/// or an untranslatable shader), or directly in a test. It holds exactly what the four triggers
/// need.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Attempt {
    /// Distinct imports the guest reached: the progress signal (D129).
    pub reached: usize,
    /// A wall this run hit, if the trace classified one.
    pub wall: Option<Wall>,
    /// Whether the guest went quiet in its own code without calling the host: a spin.
    pub spinning: bool,
}

impl Attempt {
    /// An attempt that only reached somewhere, with no wall and no spin: the ordinary case.
    #[must_use]
    pub fn reaching(reached: usize) -> Self {
        Self {
            reached,
            wall: None,
            spinning: false,
        }
    }
}

/// The record a trip produces for a person to read and act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escalation {
    /// Which trigger tripped.
    pub trigger: Trigger,
    /// The finding the loop was working on when it tripped.
    pub finding: String,
    /// How far the guest reached on the tripping attempt.
    pub reached: usize,
    /// The baseline the loop started from, for context.
    pub baseline_reached: usize,
    /// How many attempts had been made on this finding.
    pub attempts: u32,
}

impl Escalation {
    /// The `[ESCALATION-NEEDED]` block for the caller to append to the worklog.
    #[must_use]
    pub fn worklog_entry(&self) -> String {
        format!(
            concat!(
                "[ESCALATION-NEEDED] {} on `{}`\n",
                "- reached {} imports (baseline {}), after {} attempt(s) with no further.\n",
                "- the autonomous loop halted here and rolled back its inert trial patches; ",
                "a person's decision is needed."
            ),
            self.trigger.label(),
            self.finding,
            self.reached,
            self.baseline_reached,
            self.attempts,
        )
    }
}

/// The escape hatch: observes a loop's attempts on one finding and trips on the first escalation
/// condition, then stays tripped so the loop halts.
#[derive(Debug)]
pub struct EscapeHatch {
    finding: String,
    baseline_reached: usize,
    best_reached: usize,
    attempts: u32,
    consecutive_without_further: u32,
    tripped: Option<Trigger>,
}

impl EscapeHatch {
    /// Opens a hatch for one finding, with the baseline reach the loop starts from.
    #[must_use]
    pub fn new(finding: impl Into<String>, baseline_reached: usize) -> Self {
        Self {
            finding: finding.into(),
            baseline_reached,
            best_reached: baseline_reached,
            attempts: 0,
            consecutive_without_further: 0,
            tripped: None,
        }
    }

    /// Observes one attempt.
    ///
    /// Returns an [`Escalation`] the first time a trigger trips, and `None` otherwise. Once tripped
    /// the hatch is closed: every later `observe` returns `None` without advancing anything, so a
    /// caller that keeps calling it cannot un-halt the loop or double-count.
    ///
    /// A wall or a spin stops at once, since no retry passes a missing opcode or frees a deadlock.
    /// A regression is next: reaching less than the baseline is worse than doing nothing and is not
    /// retried into. Retry exhaustion is last.
    pub fn observe(&mut self, attempt: &Attempt) -> Option<Escalation> {
        if self.tripped.is_some() {
            return None;
        }
        self.attempts += 1;

        let trigger = if let Some(wall) = &attempt.wall {
            Some(Trigger::ArchitecturalWall(wall.clone()))
        } else if attempt.spinning {
            Some(Trigger::SpinDeadlock)
        } else if attempt.reached < self.baseline_reached {
            Some(Trigger::Regression)
        } else if attempt.reached > self.best_reached {
            // Further: the guest reached code it had not before, so the finding is still moving.
            self.best_reached = attempt.reached;
            self.consecutive_without_further = 0;
            None
        } else {
            self.consecutive_without_further += 1;
            (self.consecutive_without_further >= RETRY_LIMIT).then_some(Trigger::RetryExhaustion)
        };

        let trigger = trigger?;
        self.tripped = Some(trigger.clone());
        Some(Escalation {
            trigger,
            finding: self.finding.clone(),
            reached: attempt.reached,
            baseline_reached: self.baseline_reached,
            attempts: self.attempts,
        })
    }

    /// Whether the hatch has tripped, and on what: the signal a loop reads to stop.
    #[must_use]
    pub fn tripped(&self) -> Option<&Trigger> {
        self.tripped.as_ref()
    }

    /// Rolls back the inert trial patches.
    ///
    /// A patch is a proposed diff, never applied here, so rolling one back is discarding it. On a
    /// trip every patch under trial was inert, so nothing is returned and the escalation replaces
    /// them; with no trip the patches are handed straight back.
    #[must_use]
    pub fn roll_back(&self, trial_patches: Vec<Patch>) -> Vec<Patch> {
        if self.tripped.is_some() {
            Vec::new()
        } else {
            trial_patches
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Attempt, EscapeHatch, RETRY_LIMIT, Trigger, Wall};
    use crate::patch::{Evidence, Patch};

    fn a_patch() -> Patch {
        Patch {
            function: "libSceX::whatever".to_owned(),
            answers: None,
            region: None,
            evidence: Evidence::Further,
            assumptions: vec!["assumed for the test".to_owned()],
        }
    }

    /// An architectural wall trips at once, closes the hatch and rolls the trial patch back.
    #[test]
    fn an_architectural_wall_trips_and_rolls_back() {
        let mut hatch = EscapeHatch::new("libSceGnm::submit", 20);
        let escalation = hatch
            .observe(&Attempt {
                reached: 20,
                wall: Some(Wall::UnknownGpuOpcode),
                spinning: false,
            })
            .expect("a wall trips");
        assert_eq!(
            escalation.trigger,
            Trigger::ArchitecturalWall(Wall::UnknownGpuOpcode)
        );
        assert!(hatch.tripped().is_some(), "the loop has halted");
        assert!(
            hatch.roll_back(vec![a_patch()]).is_empty(),
            "the inert trial patch is rolled back"
        );
        // Closed: a further attempt does not un-halt or double-report.
        assert!(hatch.observe(&Attempt::reaching(99)).is_none());
        assert!(escalation.worklog_entry().contains("[ESCALATION-NEEDED]"));
        assert!(escalation.worklog_entry().contains("architectural wall"));
    }

    /// A spinning guest trips as a deadlock.
    #[test]
    fn a_spin_trips_as_a_deadlock() {
        let mut hatch = EscapeHatch::new("libkernel::wait", 15);
        let escalation = hatch
            .observe(&Attempt {
                reached: 15,
                wall: None,
                spinning: true,
            })
            .expect("a spin trips");
        assert_eq!(escalation.trigger, Trigger::SpinDeadlock);
        assert!(escalation.worklog_entry().contains("spin deadlock"));
    }

    /// A change that reaches less than the baseline trips as a regression.
    #[test]
    fn reaching_less_than_the_baseline_trips_as_a_regression() {
        let mut hatch = EscapeHatch::new("libSceNet::socket", 30);
        // A step forward first, then a step back below the baseline.
        assert!(hatch.observe(&Attempt::reaching(33)).is_none(), "further");
        let escalation = hatch
            .observe(&Attempt::reaching(28))
            .expect("below baseline trips");
        assert_eq!(escalation.trigger, Trigger::Regression);
    }

    /// Three attempts with no further exhaust the retries, though nothing broke.
    #[test]
    fn three_attempts_without_further_exhaust_the_retries() {
        let mut hatch = EscapeHatch::new("libSceAudioOut::output", 12);
        // Two attempts stuck at the baseline: no further, but under the limit.
        assert!(hatch.observe(&Attempt::reaching(12)).is_none());
        assert!(hatch.observe(&Attempt::reaching(12)).is_none());
        let escalation = hatch
            .observe(&Attempt::reaching(12))
            .expect("the third no-further attempt trips");
        assert_eq!(escalation.trigger, Trigger::RetryExhaustion);
        assert_eq!(escalation.attempts, RETRY_LIMIT);
    }

    /// Further resets the retry count, so a loop that is still moving does not trip.
    #[test]
    fn further_resets_the_retry_count_and_does_not_trip() {
        let mut hatch = EscapeHatch::new("libSceVideoOut::flip", 40);
        assert!(
            hatch.observe(&Attempt::reaching(40)).is_none(),
            "no further"
        );
        assert!(
            hatch.observe(&Attempt::reaching(40)).is_none(),
            "no further"
        );
        assert!(
            hatch.observe(&Attempt::reaching(41)).is_none(),
            "further - resets"
        );
        assert!(
            hatch.observe(&Attempt::reaching(41)).is_none(),
            "no further"
        );
        assert!(
            hatch.observe(&Attempt::reaching(41)).is_none(),
            "no further"
        );
        // Only the third consecutive no-further trips, so this one does.
        assert!(
            hatch.observe(&Attempt::reaching(41)).is_some(),
            "the third consecutive no-further after the reset trips"
        );
    }

    /// With no trip, patches are handed straight back to propose.
    #[test]
    fn without_a_trip_patches_are_kept() {
        let mut hatch = EscapeHatch::new("libc::malloc", 5);
        assert!(hatch.observe(&Attempt::reaching(9)).is_none(), "further");
        assert_eq!(
            hatch.roll_back(vec![a_patch()]).len(),
            1,
            "a further-making run keeps its patch"
        );
    }
}
