//! A host gamepad, read for a pad port set to [`orbistoun_input::Source::Gamepad`].
//!
//! The window owns host input (D326). On Windows the gamepads the host reports are read through
//! `gilrs`; on other hosts none is read, a gamepad port answers a neutral pad, and the run report
//! names the gap ("on the host"). The library's types stop at [`Gamepads::reading`]: what reaches
//! the pad is a [`Reading`], and turning one into a [`PadState`] is [`state_from`], tested apart
//! from any device.

use orbistoun_input::pad::TRIGGER_THRESHOLD;
use orbistoun_input::{Button, PadState, Stick};

/// One gamepad's controls as read, in nothing of the library's types.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Reading {
    /// The buttons down.
    pub(crate) pressed: Vec<Button>,
    /// Left stick, then right, as `(x, y)` with x positive right and y positive **up**, the way
    /// gamepad libraries report it.
    pub(crate) sticks: [(f32, f32); 2],
    /// Left trigger, then right, `0.0..=1.0`.
    pub(crate) triggers: [f32; 2],
}

/// The pad state a reading amounts to.
///
/// The y axes turn to the pad's own down-positive convention, sticks are clamped to the unit
/// range, and triggers go through [`PadState::set_trigger`] so value and bit agree. A trigger that
/// reports only a button, with no travel, counts as fully pulled.
pub(crate) fn state_from(reading: &Reading) -> PadState {
    let mut state = PadState::neutral();
    for &button in &reading.pressed {
        if !matches!(button, Button::L2 | Button::R2) {
            state.set(button, true);
        }
    }
    for (stick, &(x, y)) in state.sticks.iter_mut().zip(&reading.sticks) {
        *stick = Stick {
            x: x.clamp(-1.0, 1.0),
            y: (-y).clamp(-1.0, 1.0),
        };
    }
    for (right, (button, travel)) in [
        (Button::L2, reading.triggers[0]),
        (Button::R2, reading.triggers[1]),
    ]
    .into_iter()
    .enumerate()
    {
        let digital_only = reading.pressed.contains(&button) && travel < TRIGGER_THRESHOLD;
        state.set_trigger(right == 1, if digital_only { 1.0 } else { travel });
    }
    state
}

/// The host's gamepads, opened on first use.
#[derive(Default)]
pub(crate) struct Gamepads {
    #[cfg(windows)]
    host: Option<gilrs::Gilrs>,
    /// Whether opening was tried, so a host with no gamepad support is asked once.
    tried: bool,
}

impl core::fmt::Debug for Gamepads {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Gamepads")
            .field("tried", &self.tried)
            .finish_non_exhaustive()
    }
}

impl Gamepads {
    /// What the `index`th gamepad the host reports is doing, counting connected pads from zero,
    /// or `None` when there is no such pad or this host reads none.
    pub(crate) fn reading(&mut self, index: usize) -> Option<Reading> {
        self.open();
        #[cfg(windows)]
        {
            self.read(index)
        }
        // This host reads no gamepad, so every port answers a neutral pad.
        #[cfg(not(windows))]
        {
            let _ = index;
            None
        }
    }

    #[cfg(windows)]
    fn open(&mut self) {
        if self.tried {
            return;
        }
        self.tried = true;
        match gilrs::Gilrs::new() {
            // The library still works without some backends; anything else leaves pads neutral.
            Ok(host) | Err(gilrs::Error::NotImplemented(host)) => self.host = Some(host),
            Err(e) => eprintln!("orbistoun: no gamepad can be read: {e}"),
        }
    }

    #[cfg(not(windows))]
    fn open(&mut self) {
        if !self.tried {
            self.tried = true;
            eprintln!(
                "orbistoun: a pad port is set to a gamepad, and this host reads none - it answers a neutral pad"
            );
        }
    }

    #[cfg(windows)]
    fn read(&mut self, index: usize) -> Option<Reading> {
        use gilrs::{Axis, Button as Pad};

        let host = self.host.as_mut()?;
        // Pending events update the state the library holds; the state, not the events, is read.
        while host.next_event().is_some() {}
        let (_, pad) = host
            .gamepads()
            .filter(|(_, p)| p.is_connected())
            .nth(index)?;
        let pressed = [
            (Pad::South, Button::South),
            (Pad::East, Button::East),
            (Pad::West, Button::West),
            (Pad::North, Button::North),
            (Pad::LeftTrigger, Button::L1),
            (Pad::RightTrigger, Button::R1),
            (Pad::LeftTrigger2, Button::L2),
            (Pad::RightTrigger2, Button::R2),
            (Pad::LeftThumb, Button::L3),
            (Pad::RightThumb, Button::R3),
            (Pad::DPadUp, Button::Up),
            (Pad::DPadDown, Button::Down),
            (Pad::DPadLeft, Button::Left),
            (Pad::DPadRight, Button::Right),
            (Pad::Select, Button::Select),
            (Pad::Start, Button::Start),
            (Pad::Mode, Button::Shell),
        ]
        .into_iter()
        .filter(|&(from, _)| pad.is_pressed(from))
        .map(|(_, to)| to)
        .collect();
        let travel = |button| {
            pad.button_data(button)
                .map_or(0.0, gilrs::ev::state::ButtonData::value)
        };
        Some(Reading {
            pressed,
            sticks: [
                (pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY)),
                (pad.value(Axis::RightStickX), pad.value(Axis::RightStickY)),
            ],
            triggers: [travel(Pad::LeftTrigger2), travel(Pad::RightTrigger2)],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Reading, state_from};
    use orbistoun_input::Button;

    /// Buttons carry over, a stick pushed up reads up on the pad (y negative), and a trigger's
    /// travel decides its button.
    #[test]
    fn a_reading_becomes_the_pad_a_title_sees() {
        let state = state_from(&Reading {
            pressed: vec![Button::South, Button::Start],
            sticks: [(0.5, 1.0), (-2.0, -0.25)],
            triggers: [0.2, 0.9],
        });
        assert!(state.is_down(Button::South) && state.is_down(Button::Start));
        assert!(!state.is_down(Button::East));
        assert_eq!(
            (state.sticks[0].x, state.sticks[0].y),
            (0.5, -1.0),
            "up is negative y"
        );
        assert_eq!(
            (state.sticks[1].x, state.sticks[1].y),
            (-1.0, 0.25),
            "clamped, flipped"
        );
        assert!(
            !state.is_down(Button::L2),
            "a light touch is below the threshold"
        );
        assert!(state.is_down(Button::R2));
        assert!((state.triggers[1] - 0.9).abs() < f32::EPSILON);
    }

    /// A trigger that reports only its button, with no travel, is a fully pulled trigger, and a
    /// neutral reading is a neutral pad.
    #[test]
    fn a_digital_trigger_counts_as_fully_pulled() {
        let state = state_from(&Reading {
            pressed: vec![Button::L2],
            ..Reading::default()
        });
        assert!(state.is_down(Button::L2));
        assert!((state.triggers[0] - 1.0).abs() < f32::EPSILON);
        assert_eq!(
            state_from(&Reading::default()),
            orbistoun_input::PadState::neutral()
        );
    }
}
