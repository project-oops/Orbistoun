//! A scripted pad: what the pad is doing, and when.
//!
//! Titles gate on input (a licence page, a language list, press-to-start), and a run stopped at
//! one ends on the clock like a clean run. A script says what the pad does and when, so the same
//! script and guest give the same run. A step sets the pad and it stays set until the next, as
//! in [`crate::latest`]: a press and its release are two steps, and a press with no release is
//! held for the rest of the run. This module carries typed [`PadState`] and never bytes; the
//! byte layout a title reads belongs to `pad.rs` (D345).

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::pad::{Button, PadState, Stick};

/// One moment in a script: the pad's state, and when it takes effect.
///
/// When is one of two clocks (D721): `at_ms`, host milliseconds since the run started, or
/// `at_flip`, the guest's own flips since then. A flip-keyed script presses at the same point in
/// the title however fast the host runs. A step names exactly one, and a script uses one
/// throughout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// Milliseconds from the start of the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
    /// Flips the guest has made since the start of the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_flip: Option<u64>,
    /// Buttons held from this moment. Absent means none: the pad's buttons released.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<Button>,
    /// Left stick, as `[x, y]` in `-1.0..=1.0`. Absent means centred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_stick: Option<[f32; 2]>,
    /// Right stick, same units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_stick: Option<[f32; 2]>,
    /// Triggers, as `[left, right]` in `0.0..=1.0`. Absent means released.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggers: Option<[f32; 2]>,
}

impl Step {
    /// The step that sets the pad to `state` at `at_flip`, as a recording writes it (D721).
    ///
    /// Every button but the system's own, which a title never sees; an axis only when it is off
    /// its rest position.
    #[must_use]
    pub fn at_flip(at_flip: u64, state: &PadState) -> Self {
        let off = |value: f32| value != 0.0;
        let stick = |stick: Stick| (off(stick.x) || off(stick.y)).then_some([stick.x, stick.y]);
        Self {
            at_ms: None,
            at_flip: Some(at_flip),
            buttons: Button::ALL
                .into_iter()
                .filter(|&button| button != Button::Shell && state.is_down(button))
                .collect(),
            left_stick: stick(state.sticks[0]),
            right_stick: stick(state.sticks[1]),
            triggers: (off(state.triggers[0]) || off(state.triggers[1])).then_some(state.triggers),
        }
    }

    /// When it takes effect, on whichever clock it names; `None` when it names neither or both.
    fn when(&self) -> Option<(Clock, u64)> {
        match (self.at_ms, self.at_flip) {
            (Some(ms), None) => Some((Clock::Millis, ms)),
            (None, Some(flip)) => Some((Clock::Flips, flip)),
            _ => None,
        }
    }
}

/// What a script's times count (D721).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    /// Milliseconds of host time since the run started.
    Millis,
    /// The guest's flips since the run started.
    Flips,
}

impl Clock {
    const fn unit(self) -> &'static str {
        match self {
            Self::Millis => "ms",
            Self::Flips => " flips",
        }
    }
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
    /// Refused rather than sorted: sorting would silently run something other than what the file
    /// says.
    OutOfOrder {
        /// The index of the offending step.
        step: usize,
        /// The time it claims.
        at: u64,
        /// The time of the step before it.
        after: u64,
        /// What the times count.
        clock: Clock,
    },
    /// A step names no time, or both an `at_ms` and an `at_flip` (D721).
    NoTime {
        /// The index of the offending step.
        step: usize,
    },
    /// A step counts a different clock from the steps before it (D721). Refused rather than
    /// merged, since "the fifth flip or two seconds, whichever comes first" is not an order.
    MixedClocks {
        /// The index of the offending step.
        step: usize,
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
                at,
                after,
                clock,
            } => write!(
                f,
                // `concat!` of one-line literals rather than a `\`-continued literal, which `cargo fmt` would
                // collapse with the source indentation baked in. It defeats implicit capture, so the arguments
                // are positional.
                concat!(
                    "step {} is at {}{}, which is not after the {}{} before it - ",
                    "order the steps as they run"
                ),
                step,
                at,
                clock.unit(),
                after,
                clock.unit()
            ),
            Self::NoTime { step } => write!(
                f,
                "step {step} names no time, or both - give it `at_ms` or `at_flip`, one of them"
            ),
            Self::MixedClocks { step } => write!(
                f,
                "step {step} counts a different clock from the steps before it - use `at_ms` or `at_flip` throughout"
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
    /// Deserialising is the caller's job, as in `mapping.rs`: the types derive `Deserialize` and
    /// the owner of the file format reads it, so this crate has no format dependency. Whether the
    /// steps make sense is checked here.
    ///
    /// # Errors
    ///
    /// [`ScriptError`] when the steps are not in order, or an axis is outside its range.
    pub fn validate(&self) -> Result<(), ScriptError> {
        let mut previous: Option<(Clock, u64)> = None;
        for (index, step) in self.steps.iter().enumerate() {
            let (clock, at) = step.when().ok_or(ScriptError::NoTime { step: index })?;
            if let Some((before_clock, after)) = previous {
                if clock != before_clock {
                    return Err(ScriptError::MixedClocks { step: index });
                }
                if at <= after {
                    return Err(ScriptError::OutOfOrder {
                        step: index,
                        at,
                        after,
                        clock,
                    });
                }
            }
            previous = Some((clock, at));

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

    /// When the last step is, on the script's clock, or `None` for an empty script. After it the
    /// pad holds its final state.
    #[must_use]
    pub fn last_at(&self) -> Option<u64> {
        self.steps.last().and_then(Step::when).map(|(_, at)| at)
    }

    /// What its times count: milliseconds for an empty script, which has none.
    #[must_use]
    pub fn clock(&self) -> Clock {
        self.steps
            .first()
            .and_then(Step::when)
            .map_or(Clock::Millis, |(clock, _)| clock)
    }

    /// The pad's state at `elapsed` on the script's clock: the most recent step at or before it.
    ///
    /// Before the first step the pad is at rest, the same as with no script.
    #[must_use]
    pub fn at(&self, elapsed: u64) -> PadState {
        let mut state = PadState::default();
        // `validate` guarantees order, so the step in force is found by bisection.
        let past = self
            .steps
            .partition_point(|step| step.when().is_some_and(|(_, at)| at <= elapsed));
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
/// A static, like [`crate::latest`]'s ports: the guest calls the pad shim from its own threads
/// with no context to carry.
static ACTIVE: Mutex<Option<Playing>> = Mutex::new(None);

/// A script playing, and where both of its possible clocks stood when it started.
#[derive(Debug)]
struct Playing {
    script: Script,
    started: Instant,
    started_flip: u64,
}

/// The guest's flip count, handed in by whoever can see the video shim (D721).
static FLIPS: OnceLock<fn() -> u64> = OnceLock::new();

/// Installs where the guest's flip count is read from (`orbistoun_video::flips_accepted` in the
/// worker), as a function so neither crate depends on the other. The first install wins.
pub fn install_flip_clock(flips: fn() -> u64) {
    let _ = FLIPS.set(flips);
}

/// The guest's flips so far; zero where no clock was installed, so a flip-keyed script holds its
/// opening state rather than running ahead.
fn flips() -> u64 {
    FLIPS.get().map_or(0, |flips| flips())
}

/// Starts a script playing from now, replacing any script already installed.
pub fn install(script: Script) {
    *lock() = Some(Playing {
        script,
        started: Instant::now(),
        started_flip: flips(),
    });
}

/// Stops whatever was playing, so a later run does not inherit it.
pub fn clear() {
    *lock() = None;
}

/// What the installed script says the pad is doing now, or `None` when none is installed.
///
/// Sampled on demand, not pushed by a timer: the state is a pure function of run progress in
/// milliseconds or flips (D721), so no thread races the guest's polling.
#[must_use]
pub fn poll() -> Option<PadState> {
    let held = lock();
    let playing = held.as_ref()?;
    let elapsed = match playing.script.clock() {
        Clock::Millis => u64::try_from(playing.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        Clock::Flips => flips().saturating_sub(playing.started_flip),
    };
    Some(playing.script.at(elapsed))
}

/// The guard, with a poisoned lock treated as ordinary, as in [`crate::latest`]: what is behind
/// it has no invariant a partial write could break.
fn lock() -> std::sync::MutexGuard<'static, Option<Playing>> {
    ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A run being recorded (D721): where each step goes, the last state written, and the flip the
/// recording counts from.
struct Recording {
    sink: fn(&Step),
    last: Option<PadState>,
    started_flip: u64,
    /// The flip the last step was written at.
    last_at: Option<u64>,
}

static RECORDING: Mutex<Option<Recording>> = Mutex::new(None);

/// Records what the guest reads from now on, a step to `sink` each time it differs from the
/// last (D721).
///
/// The steps are a flip-timed script. `sink` is called as each step happens, so a recording
/// survives a run that ends in a fault.
pub fn record(sink: fn(&Step)) {
    *RECORDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Recording {
        sink,
        last: None,
        started_flip: flips(),
        last_at: None,
    });
}

/// Stops recording, so a later run does not append to this one.
pub fn stop_recording() {
    *RECORDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// The pad state the guest was just handed, recorded as a step when it differs from the last.
pub fn delivered(state: &PadState) {
    let mut held = RECORDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(recording) = held.as_mut() else {
        return;
    };
    if recording.last.as_ref() == Some(state) {
        return;
    }
    recording.last = Some(*state);
    // A title reads its pad several times a frame, so two changes can fall in one flip. The later
    // goes to the next flip rather than being dropped, so every state the guest saw is kept.
    let now = flips().saturating_sub(recording.started_flip);
    let at = recording.last_at.map_or(now, |last| now.max(last + 1));
    recording.last_at = Some(at);
    (recording.sink)(&Step::at_flip(at, state));
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

    /// A step takes effect at its own time and holds until the next.
    ///
    /// Checked at the boundaries, where `<` and `<=` differ.
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

    /// A press with no release is held for the rest of the run.
    #[test]
    fn a_press_never_released_is_held_to_the_end() {
        let script = script("[[step]]\nat_ms = 50\nbuttons = [\"start\"]\n")
            .expect("a one-step script is well formed");
        assert!(!script.at(49).is_down(Button::Start));
        assert!(script.at(50).is_down(Button::Start));
        assert!(script.at(u64::MAX).is_down(Button::Start));
        assert_eq!(script.last_at(), Some(50));
    }

    /// An empty script is a run with no input, not an error.
    #[test]
    fn an_empty_script_is_a_pad_at_rest() {
        let script = script("").expect("an empty script is well formed");
        assert!(script.is_empty());
        assert_eq!(script.last_at(), None);
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

        // Compared as bits: the values pass through unchanged, and an epsilon would also accept a
        // rescaling.
        let state = script.at(0);
        let same = |got: f32, want: f32| got.to_bits() == want.to_bits();
        assert!(same(state.sticks[0].x, -1.0), "left stick x");
        assert!(same(state.sticks[0].y, 0.5), "left stick y");
        assert!(same(state.sticks[1].x, 0.25), "right stick x");
        assert!(same(state.sticks[1].y, -0.75), "right stick y");
        assert!(same(state.triggers[0], 0.0), "left trigger");
        assert!(same(state.triggers[1], 1.0), "right trigger");
    }

    /// Steps out of order, or at the same instant, are refused rather than sorted.
    #[test]
    fn steps_out_of_order_are_refused_rather_than_sorted() {
        let backwards =
            script("[[step]]\nat_ms = 200\n\n[[step]]\nat_ms = 100\n").expect_err("goes backwards");
        assert_eq!(
            backwards,
            ScriptError::OutOfOrder {
                step: 1,
                at: 100,
                after: 200,
                clock: super::Clock::Millis,
            }
        );

        let duplicated =
            script("[[step]]\nat_ms = 100\n\n[[step]]\nat_ms = 100\n").expect_err("same instant");
        assert!(matches!(
            duplicated,
            ScriptError::OutOfOrder { step: 1, .. }
        ));
    }

    /// A flip-keyed step takes effect at its flip, and a step names exactly one clock, the same
    /// as the steps before it (D721).
    #[test]
    fn flip_steps_take_effect_at_their_flip_and_one_clock_is_used_throughout() {
        let flips = script(
            "[[step]]\nat_flip = 3\nbuttons = [\"start\"]\n\n[[step]]\nat_flip = 5\nbuttons = []\n",
        )
        .expect("a flip script is well formed");
        assert_eq!(flips.clock(), super::Clock::Flips);
        assert!(!flips.at(2).is_down(Button::Start));
        assert!(flips.at(3).is_down(Button::Start));
        assert!(!flips.at(5).is_down(Button::Start));

        assert_eq!(
            script("[[step]]\nbuttons = [\"start\"]\n").expect_err("no time"),
            ScriptError::NoTime { step: 0 }
        );
        assert_eq!(
            script("[[step]]\nat_ms = 1\nat_flip = 1\n").expect_err("both times"),
            ScriptError::NoTime { step: 0 }
        );
        assert_eq!(
            script("[[step]]\nat_flip = 1\n\n[[step]]\nat_ms = 2\n").expect_err("two clocks"),
            ScriptError::MixedClocks { step: 1 }
        );
    }

    /// A recorded step replays as the state it was recorded from, without the system button
    /// (D721).
    #[test]
    fn a_recorded_step_replays_as_the_state_it_was_recorded_from() {
        let mut state = crate::pad::PadState::default();
        state.set(Button::South, true);
        state.set(Button::Shell, true);
        state.sticks[0] = crate::pad::Stick { x: -0.5, y: 0.25 };
        state.set_trigger(true, 0.75);
        let step = super::Step::at_flip(7, &state);
        let text = format!(
            "[[step]]\n{}",
            toml::to_string(&step).expect("a step writes")
        );
        let replayed = script(&text).expect("a recorded step reads back");
        let mut expected = state;
        expected.set(Button::Shell, false);
        assert_eq!(replayed.at(7), expected);
        assert_eq!(replayed.at(6), crate::pad::PadState::default());
    }

    /// An axis outside its own range is refused: `-0.5` is valid for a stick and not for a
    /// trigger.
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
    /// Each holds this from its first `clear` through its last assertion so no other test installs
    /// in between; poisoning is recovered from so one panic does not strand the rest.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Reads and checks a script the way a caller would.
    fn script(text: &str) -> Script {
        let script: Script = toml::from_str(text).expect("the test's own TOML parses");
        script.validate().expect("the test's own script is valid");
        script
    }

    /// An installed script is sampled, a second install replaces it, and a cleared one is not.
    #[test]
    fn an_installed_script_is_sampled_and_a_cleared_one_is_not() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        clear();
        assert!(
            poll().is_none(),
            "with nothing installed there is nothing to sample"
        );

        // A step at zero is in force immediately, so no sleep or clock control is needed.
        install(script("[[step]]\nat_ms = 0\nbuttons = [\"south\"]\n"));
        let sampled = poll().expect("an installed script samples");
        assert!(
            sampled.is_down(Button::South),
            "the step in force at the start is the one at zero"
        );

        // Installing again replaces rather than merges.
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

    /// A recording writes a step when what the guest reads changes, stamped with the flip, and only
    /// then; a flip script installed afterwards plays against the same clock (D721).
    #[test]
    fn a_recording_writes_a_step_per_change_at_its_flip() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static FLIP: AtomicU64 = AtomicU64::new(0);
        static WRITTEN: Mutex<Vec<super::Step>> = Mutex::new(Vec::new());
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        super::install_flip_clock(|| FLIP.load(Ordering::SeqCst));
        FLIP.store(10, Ordering::SeqCst);
        WRITTEN.lock().expect("steps").clear();
        super::record(|step| WRITTEN.lock().expect("steps").push(step.clone()));

        let mut pressed = crate::pad::PadState::default();
        pressed.set(Button::Start, true);
        super::delivered(&crate::pad::PadState::default());
        FLIP.store(12, Ordering::SeqCst);
        super::delivered(&crate::pad::PadState::default());
        super::delivered(&pressed);
        FLIP.store(13, Ordering::SeqCst);
        super::delivered(&pressed);
        // Released and pressed again inside flip 13: both kept, the second a flip later, so the script
        // stays in order.
        super::delivered(&crate::pad::PadState::default());
        super::delivered(&pressed);
        super::stop_recording();
        super::delivered(&crate::pad::PadState::default());

        let written = WRITTEN.lock().expect("steps").clone();
        let at: Vec<Option<u64>> = written.iter().map(|step| step.at_flip).collect();
        assert_eq!(
            at,
            [Some(0), Some(2), Some(3), Some(4)],
            "one step per change, from the recording's start, never two at one flip"
        );
        assert_eq!(written[1].buttons, [Button::Start]);

        // Played back from flip 20: its second step lands two flips in.
        FLIP.store(20, Ordering::SeqCst);
        install(script(&format!(
            "[[step]]\n{}\n[[step]]\n{}",
            toml::to_string(&written[0]).expect("writes"),
            toml::to_string(&written[1]).expect("writes")
        )));
        FLIP.store(21, Ordering::SeqCst);
        assert!(!poll().expect("playing").is_down(Button::Start));
        FLIP.store(22, Ordering::SeqCst);
        assert!(poll().expect("playing").is_down(Button::Start));
        clear();
    }

    /// A step in the future is not in force at the start of a run, checked through the installed
    /// path so a lost start instant fails here.
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
