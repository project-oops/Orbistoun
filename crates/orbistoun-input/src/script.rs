//! A scripted pad: what the pad is doing, and when.
//!
//! # Why a title needs one
//!
//! Titles gate on input - a licence page, a language list, a press-to-start. A guest stopped at
//! one of those and a conformance probe that ran clean both end the same way, on the clock, and
//! the compatibility table cannot tell them apart (`REQ-20260915T0929Z-02a0`). Nothing in the
//! tree could press a button, so no run had ever got past a prompt.
//!
//! This is the deterministic half of the answer: a file that says what the pad does and when,
//! so a run is repeatable. The same script and the same guest give the same run, which is what
//! makes a compatibility result mean anything.
//!
//! # A level, not a stream
//!
//! A step **sets** the pad and it stays set until the next step. That follows
//! [`crate::latest`], which keeps the most recent state per port rather than a queue, for the
//! reason recorded there: a title asks what the pad is doing *now*, and replaying a backlog of
//! finished presses is worse than nothing.
//!
//! So a press and its release are two steps, and the gap between them is how long it was held.
//! Writing only the press means holding it for the rest of the run, which is a real thing to
//! want and so is not an error.
//!
//! # What this deliberately does not decide
//!
//! **Which bytes a title reads to see any of this is not settled here and cannot be.** The
//! 120-byte extent and its at-rest contents are measured; which offset inside carries the
//! buttons is an inference from one at-rest image (`crate::latest`, D345), and the measurement
//! that would settle it needs somebody holding a button on real hardware
//! (obSCEne `REQ-20260910T0650Z-d1c4`, open since 2026-09-10 and pending on exactly that).
//!
//! That separation is the point rather than a limitation. This module carries [`PadState`] -
//! typed buttons, sticks and triggers - and never bytes. When the encoding is measured, what
//! changes is `pad.rs`, and every script written before it keeps working unaltered.

use std::sync::Mutex;
use std::time::Instant;

use serde::Deserialize;

use crate::pad::{Button, PadState, Stick};

/// One moment in a script: the pad's state, and when it takes effect.
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    /// Milliseconds from the start of the run.
    pub at_ms: u64,
    /// Buttons held from this moment. Absent means none - the pad's buttons released.
    #[serde(default)]
    pub buttons: Vec<Button>,
    /// Left stick, as `[x, y]` in `-1.0..=1.0`. Absent means centred.
    #[serde(default)]
    pub left_stick: Option<[f32; 2]>,
    /// Right stick, same units.
    #[serde(default)]
    pub right_stick: Option<[f32; 2]>,
    /// Triggers, as `[left, right]` in `0.0..=1.0`. Absent means released.
    #[serde(default)]
    pub triggers: Option<[f32; 2]>,
}

/// A whole script, in the order it runs.
#[derive(Debug, Clone, Deserialize)]
pub struct Script {
    /// The steps, earliest first.
    #[serde(default, rename = "step")]
    steps: Vec<Step>,
}

/// Why a script was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptError {
    /// Two steps share a time, or a later one comes first.
    ///
    /// Refused rather than sorted. A script whose steps are out of order is a mistake somebody
    /// made, and sorting it silently would run something other than what the file says while
    /// looking like it worked - which is the failure this whole subsystem exists to make
    /// visible rather than commit.
    OutOfOrder {
        /// The index of the offending step.
        step: usize,
        /// The time it claims.
        at_ms: u64,
        /// The time of the step before it.
        after_ms: u64,
    },
    /// A stick or trigger is outside the range its axis can express.
    OutOfRange {
        /// The index of the offending step.
        step: usize,
        /// Which field.
        field: &'static str,
        /// What it said.
        value: f32,
    },
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfOrder {
                step,
                at_ms,
                after_ms,
            } => write!(
                f,
                // `concat!` of one-line literals rather than a `\`-continued one, which `cargo
                // fmt` would collapse with the source indentation baked into the message
                // (D184, D199). It defeats implicit capture, so the arguments are positional.
                concat!(
                    "step {} is at {}ms, which is not after the {}ms before it - ",
                    "order the steps as they run"
                ),
                step, at_ms, after_ms
            ),
            Self::OutOfRange { step, field, value } => {
                write!(f, "step {step}'s {field} is {value}, outside its range")
            }
        }
    }
}

impl std::error::Error for ScriptError {}

impl Script {
    /// Refuses a script that does not describe a run that could happen.
    ///
    /// **Deserialising is the caller's job, not this crate's.** `mapping.rs` settled that
    /// shape already: the types here derive `Deserialize` and whoever owns the file format
    /// reads it, so this crate needs no format dependency and a script could arrive as TOML,
    /// as a settings field, or over the shim-to-worker protocol without this module caring.
    /// What cannot be delegated is whether the steps make sense, so that is here.
    ///
    /// # Errors
    ///
    /// [`ScriptError`] when the steps are not in order, or an axis is outside its range.
    pub fn validate(&self) -> Result<(), ScriptError> {
        let mut previous: Option<u64> = None;
        for (index, step) in self.steps.iter().enumerate() {
            if let Some(after_ms) = previous
                && step.at_ms <= after_ms
            {
                return Err(ScriptError::OutOfOrder {
                    step: index,
                    at_ms: step.at_ms,
                    after_ms,
                });
            }
            previous = Some(step.at_ms);

            for (field, axes) in [
                ("left_stick", step.left_stick),
                ("right_stick", step.right_stick),
            ] {
                if let Some(axes) = axes {
                    for value in axes {
                        if !(-1.0..=1.0).contains(&value) {
                            return Err(ScriptError::OutOfRange {
                                step: index,
                                field,
                                value,
                            });
                        }
                    }
                }
            }
            if let Some(triggers) = step.triggers {
                for value in triggers {
                    if !(0.0..=1.0).contains(&value) {
                        return Err(ScriptError::OutOfRange {
                            step: index,
                            field: "triggers",
                            value,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// How many steps it has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether it says to do nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// When the last step is, or `None` for an empty script.
    ///
    /// What a caller needs to know how long the script has left to say anything - after this
    /// the pad holds its final state, so a run going on longer is not waiting for the script.
    #[must_use]
    pub fn last_at_ms(&self) -> Option<u64> {
        self.steps.last().map(|step| step.at_ms)
    }

    /// The pad's state at `elapsed_ms`: the most recent step at or before it.
    ///
    /// Before the first step the pad is at rest, which is what a title sees while it starts up
    /// and is the same thing an absent script gives.
    #[must_use]
    pub fn at(&self, elapsed_ms: u64) -> PadState {
        let mut state = PadState::default();
        // The steps are in order - `validate` refuses a script where they are not - so the one
        // in force is the last at or before now, found by bisection rather than a scan.
        let past = self.steps.partition_point(|step| step.at_ms <= elapsed_ms);
        let Some(step) = past.checked_sub(1).map(|index| &self.steps[index]) else {
            return state;
        };

        for &button in &step.buttons {
            state.set(button, true);
        }
        if let Some([x, y]) = step.left_stick {
            state.sticks[0] = Stick { x, y };
        }
        if let Some([x, y]) = step.right_stick {
            state.sticks[1] = Stick { x, y };
        }
        if let Some([left, right]) = step.triggers {
            state.set_trigger(false, left);
            state.set_trigger(true, right);
        }
        state
    }
}

/// The script this run is playing, and when it started.
///
/// A static for the same reason [`crate::latest`]'s ports are one: the guest calls the pad shim
/// from its own threads with no context to carry, so what the run decided has to be reachable
/// from there.
static ACTIVE: Mutex<Option<(Script, Instant)>> = Mutex::new(None);

/// Starts a script playing from now.
///
/// Replaces any script already installed, which is what a second run in one process means.
pub fn install(script: Script) {
    *lock() = Some((script, Instant::now()));
}

/// Stops whatever was playing, so a later run does not inherit it.
pub fn clear() {
    *lock() = None;
}

/// What the installed script says the pad is doing now, or `None` when none is installed.
///
/// **Sampled on demand rather than pushed by a timer**, which is what makes a run repeatable:
/// the state is a pure function of how long the run has been going, so there is no thread to
/// race with and no tick rate to drift against the guest's own polling.
#[must_use]
pub fn poll() -> Option<PadState> {
    let held = lock();
    let (script, started) = held.as_ref()?;
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Some(script.at(elapsed))
}

/// The guard, with a poisoned lock treated as ordinary - as [`crate::latest`] does, and for the
/// same reason: what is behind it has no invariant a partial write could break.
fn lock() -> std::sync::MutexGuard<'static, Option<(Script, Instant)>> {
    ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{Script, ScriptError};
    use crate::pad::Button;

    /// Reads a script the way a caller would, then checks it.
    fn script(text: &str) -> Result<Script, ScriptError> {
        let script: Script = toml::from_str(text).expect("the test's own TOML parses");
        script.validate()?;
        Ok(script)
    }

    /// **A step holds until the next one**, which is what makes a press have a duration.
    ///
    /// Checked at the boundaries rather than in the middle of each span: the moment a step
    /// names is the moment it takes effect, and the millisecond before it still belongs to the
    /// step before. An implementation using `<` instead of `<=` passes a mid-span check and
    /// fails here.
    #[test]
    fn a_step_takes_effect_at_its_own_time_and_holds_until_the_next() {
        let script = script(
            "[[step]]\nat_ms = 100\nbuttons = [\"south\"]\n\n[[step]]\nat_ms = 300\nbuttons = []\n",
        )
        .expect("a two-step script is well formed");

        let down = |at| script.at(at).is_down(Button::South);
        assert!(!down(0), "before the first step the pad is at rest");
        assert!(
            !down(99),
            "the millisecond before a step belongs to the one before it"
        );
        assert!(down(100), "a step takes effect at the time it names");
        assert!(down(299), "and holds right up to the next");
        assert!(!down(300), "which releases it at its own time");
        assert!(
            !down(10_000),
            "and the last step holds for the rest of the run"
        );
    }

    /// A press with no release is held for the rest of the run, deliberately.
    #[test]
    fn a_press_never_released_is_held_to_the_end() {
        let script = script("[[step]]\nat_ms = 50\nbuttons = [\"start\"]\n")
            .expect("a one-step script is well formed");
        assert!(!script.at(49).is_down(Button::Start));
        assert!(script.at(50).is_down(Button::Start));
        assert!(script.at(u64::MAX).is_down(Button::Start));
        assert_eq!(script.last_at_ms(), Some(50));
    }

    /// An empty script is a run with no input, not an error.
    #[test]
    fn an_empty_script_is_a_pad_at_rest() {
        let script = script("").expect("an empty script is well formed");
        assert!(script.is_empty());
        assert_eq!(script.last_at_ms(), None);
        assert_eq!(script.at(0), crate::pad::PadState::default());
        assert_eq!(script.at(9_999), crate::pad::PadState::default());
    }

    /// Sticks and triggers travel too, not just buttons.
    #[test]
    fn the_analogue_axes_arrive_as_written() {
        let script = script(concat!(
            "[[step]]\n",
            "at_ms = 0\n",
            "left_stick = [-1.0, 0.5]\n",
            "right_stick = [0.25, -0.75]\n",
            "triggers = [0.0, 1.0]\n",
        ))
        .expect("an analogue step is well formed");

        // Compared as bits rather than as floats, and not within an epsilon: these values are
        // carried through unchanged, so **exactly unchanged** is the property, and an epsilon
        // would also pass for an implementation that quietly rescaled them.
        let state = script.at(0);
        let same = |got: f32, want: f32| got.to_bits() == want.to_bits();
        assert!(same(state.sticks[0].x, -1.0), "left stick x");
        assert!(same(state.sticks[0].y, 0.5), "left stick y");
        assert!(same(state.sticks[1].x, 0.25), "right stick x");
        assert!(same(state.sticks[1].y, -0.75), "right stick y");
        assert!(same(state.triggers[0], 0.0), "left trigger");
        assert!(same(state.triggers[1], 1.0), "right trigger");
    }

    /// **Steps out of order are refused, not sorted.**
    ///
    /// Sorting would run something other than what the file says while looking like it worked.
    /// Both shapes are checked - a step that goes backwards, and two at the same instant, which
    /// is ambiguous rather than merely out of order.
    #[test]
    fn steps_out_of_order_are_refused_rather_than_sorted() {
        let backwards =
            script("[[step]]\nat_ms = 200\n\n[[step]]\nat_ms = 100\n").expect_err("goes backwards");
        assert_eq!(
            backwards,
            ScriptError::OutOfOrder {
                step: 1,
                at_ms: 100,
                after_ms: 200
            }
        );

        let duplicated =
            script("[[step]]\nat_ms = 100\n\n[[step]]\nat_ms = 100\n").expect_err("same instant");
        assert!(matches!(
            duplicated,
            ScriptError::OutOfOrder { step: 1, .. }
        ));
    }

    /// An axis outside its range is refused, and the two ranges are different.
    ///
    /// A stick is bipolar and a trigger is not, so `-0.5` is ordinary for one and impossible
    /// for the other. A check that used one range for both would pass the stick case here and
    /// fail the trigger one.
    #[test]
    fn an_axis_outside_its_own_range_is_refused() {
        assert!(script("[[step]]\nat_ms = 0\nleft_stick = [-0.5, 0.5]\n").is_ok());
        assert!(script("[[step]]\nat_ms = 0\ntriggers = [-0.5, 0.0]\n").is_err());

        let far = script("[[step]]\nat_ms = 0\nright_stick = [0.0, 1.5]\n")
            .expect_err("a stick cannot travel past 1.0");
        assert!(matches!(
            far,
            ScriptError::OutOfRange {
                step: 0,
                field: "right_stick",
                ..
            }
        ));
    }
}

#[cfg(test)]
mod active_tests {
    use super::{Script, clear, install, poll};
    use crate::pad::Button;
    use std::sync::{Mutex, PoisonError};

    /// Serialises the tests that share the process-wide `ACTIVE` script.
    ///
    /// `ACTIVE` is a static, so two of these running at once install over each other - one test's
    /// `clear` landing between another's `install` and `poll` - and fail for a reason that has
    /// nothing to do with what they check, which reddened the gate once. Each holds this from its
    /// first `clear` through its last assertion, so no other interleaves; poisoning is recovered
    /// from so one test's panic does not strand the rest.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Reads and checks a script the way a caller would.
    fn script(text: &str) -> Script {
        let script: Script = toml::from_str(text).expect("the test's own TOML parses");
        script.validate().expect("the test's own script is valid");
        script
    }

    #[test]
    fn an_installed_script_is_sampled_and_a_cleared_one_is_not() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        clear();
        assert!(
            poll().is_none(),
            "with nothing installed there is nothing to sample"
        );

        // A step at zero is in force immediately, so this needs no sleep and no clock control:
        // whatever the elapsed time is, it is at least zero.
        install(script("[[step]]\nat_ms = 0\nbuttons = [\"south\"]\n"));
        let sampled = poll().expect("an installed script samples");
        assert!(
            sampled.is_down(Button::South),
            "the step in force at the start is the one at zero"
        );

        // Installing again replaces rather than merges, which is what a second run means.
        install(script("[[step]]\nat_ms = 0\nbuttons = [\"north\"]\n"));
        let replaced = poll().expect("the replacement samples");
        assert!(
            replaced.is_down(Button::North),
            "the new script is in force"
        );
        assert!(
            !replaced.is_down(Button::South),
            "and the old one is gone rather than merged"
        );

        clear();
        assert!(poll().is_none(), "clearing stops a later run inheriting it");
    }

    /// A step in the future is not in force at the start of a run.
    ///
    /// The other half of the boundary, checked through the installed path rather than through
    /// `Script::at` directly, so an `install` that lost the start instant would fail here.
    #[test]
    fn a_step_in_the_future_has_not_happened_yet() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        clear();
        install(script("[[step]]\nat_ms = 3600000\nbuttons = [\"start\"]\n"));
        let sampled = poll().expect("an installed script samples");
        assert!(
            !sampled.is_down(Button::Start),
            "a step an hour away is not in force in the first milliseconds"
        );
        clear();
    }
}
