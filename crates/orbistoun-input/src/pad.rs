//! What a controller is, in our own terms.
//!
//! # Named by position, never by glyph
//!
//! Principle 2. The face buttons carry vendor symbols and the two centre buttons carry
//! vendor words, and none of those belong in this tree - so a button is *where it is*.
//! [`Button::South`] is the lower face button on every pad ever made, which is more than
//! can be said for any of its names.
//!
//! It is also the only naming that survives the thing this crate exists to do. A keyboard
//! has no glyphs on it, so a mapping from a key to a button has to be a mapping to a
//! position; and a host gamepad reports positions too. Naming by glyph would mean
//! translating twice for no gain.
//!
//! # Why the state is host-shaped
//!
//! Floats in a settled range rather than whatever the vendor packs into its structure.
//! **The guest-facing layout is unmeasured** and this type deliberately does not guess at
//! it: it describes what a person is doing with their hands, which is knowable, and the
//! conversion to whatever a title reads is a separate problem that begins with a
//! measurement (D326).

use serde::{Deserialize, Serialize};

/// One button, by where it sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Button {
    /// Lower face button.
    South,
    /// Right face button.
    East,
    /// Left face button.
    West,
    /// Upper face button.
    North,
    /// Upper left shoulder.
    L1,
    /// Upper right shoulder.
    R1,
    /// Lower left shoulder. Analogue on most pads; see [`PadState::triggers`].
    L2,
    /// Lower right shoulder.
    R2,
    /// Left stick pressed in.
    L3,
    /// Right stick pressed in.
    R3,
    /// Directional pad.
    Up,
    /// Directional pad.
    Down,
    /// Directional pad.
    Left,
    /// Directional pad.
    Right,
    /// Left centre button.
    Select,
    /// Right centre button.
    Start,
    /// The button that belongs to the system rather than to the title.
    ///
    /// **The only one with different rules.** Every other button is the title's when the
    /// title has focus; this one is the shell's always, because it is how somebody reaches
    /// the shell *from* a title. A title never sees it (D326).
    Shell,
}

impl Button {
    /// Every button, for iterating a mapping.
    pub const ALL: [Self; 17] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::L1,
        Self::R1,
        Self::L2,
        Self::R2,
        Self::L3,
        Self::R3,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::Select,
        Self::Start,
        Self::Shell,
    ];

    /// The name used in a mapping file and in the settings window.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
            Self::North => "north",
            Self::L1 => "l1",
            Self::R1 => "r1",
            Self::L2 => "l2",
            Self::R2 => "r2",
            Self::L3 => "l3",
            Self::R3 => "r3",
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Select => "select",
            Self::Start => "start",
            Self::Shell => "shell",
        }
    }

    /// Its place in the pressed-set.
    #[must_use]
    pub fn bit(self) -> u32 {
        1_u32 << (self as u32)
    }
}

/// Which way a stick is pushed.
///
/// Both axes in `-1.0..=1.0`, positive right and positive down - the convention every
/// windowing system on this host already uses, so a source does not have to flip one axis
/// and then have somebody wonder later which one was flipped.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Stick {
    /// Left to right.
    pub x: f32,
    /// Up to down.
    pub y: f32,
}

/// What somebody is doing with a pad right now.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PadState {
    /// Which buttons are down, by [`Button::bit`].
    pressed: u32,
    /// Left stick, then right.
    pub sticks: [Stick; 2],
    /// Left trigger, then right, in `0.0..=1.0`.
    ///
    /// Held apart from `L2`/`R2` rather than derived from them: a trigger at rest is not
    /// the same as a trigger held a third of the way, and a title that reads the analogue
    /// value would be told a lie by a bit that only says "past the threshold".
    pub triggers: [f32; 2],
}

/// How far a trigger travels before it counts as its button being down.
///
/// A guess, and a small one. Nothing here has measured what the hardware uses, and the
/// consequence of being wrong is a button that engages slightly early or late rather than
/// a title reading something impossible.
pub const TRIGGER_THRESHOLD: f32 = 0.5;

impl PadState {
    /// Nothing pressed, sticks centred.
    ///
    /// **What a title is handed when the shell has focus.** Not "no controller", which is a
    /// different claim and a worse one - a title that loses its pad will often stop or put
    /// up a prompt, where a pad with nothing pressed is an ordinary quiet frame.
    #[must_use]
    pub fn neutral() -> Self {
        Self::default()
    }

    /// Whether a button is down.
    #[must_use]
    pub fn is_down(&self, button: Button) -> bool {
        self.pressed & button.bit() != 0
    }

    /// Presses or releases a button.
    pub fn set(&mut self, button: Button, down: bool) {
        if down {
            self.pressed |= button.bit();
        } else {
            self.pressed &= !button.bit();
        }
    }

    /// Sets a trigger, and the button that follows from it.
    ///
    /// One call rather than two, so the analogue value and the bit **cannot disagree**.
    /// Setting them separately is how a pad ends up reporting `L2` down at a travel of
    /// zero, which no hardware does and every title is entitled to assume cannot happen.
    pub fn set_trigger(&mut self, right: bool, travel: f32) {
        let travel = travel.clamp(0.0, 1.0);
        self.triggers[usize::from(right)] = travel;
        self.set(
            if right { Button::R2 } else { Button::L2 },
            travel >= TRIGGER_THRESHOLD,
        );
    }

    /// Everything down, as a set - for a title that reads buttons in bulk.
    #[must_use]
    pub fn pressed(&self) -> u32 {
        self.pressed
    }

    /// The same state with the shell's own button removed.
    ///
    /// **What a title is allowed to see.** The shell button is how somebody reaches the
    /// shell, so a title that could observe it could also act on it - and a title that
    /// pauses itself when the shell opens is fine, while one that reads it as its own
    /// input is a title responding to a press meant for something else (D326).
    #[must_use]
    pub fn as_title_sees_it(&self) -> Self {
        let mut seen = *self;
        seen.set(Button::Shell, false);
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, PadState, TRIGGER_THRESHOLD};

    /// A neutral pad is quiet, not absent.
    #[test]
    fn a_neutral_pad_has_nothing_pressed_and_centred_sticks() {
        let pad = PadState::neutral();
        assert_eq!(pad.pressed(), 0);
        assert!(pad.sticks[0].x.abs() < f32::EPSILON);
        assert!(pad.triggers.iter().all(|t| t.abs() < f32::EPSILON));
    }

    /// Buttons go down and come back up independently.
    #[test]
    fn buttons_are_independent() {
        let mut pad = PadState::neutral();
        pad.set(Button::South, true);
        pad.set(Button::North, true);
        pad.set(Button::South, false);

        assert!(!pad.is_down(Button::South));
        assert!(pad.is_down(Button::North));
    }

    /// **Every button has its own bit.**
    ///
    /// Asserted rather than assumed: the set is a shift by the discriminant, so a variant
    /// inserted in the middle silently renumbers everything after it, and two buttons
    /// sharing a bit would present as one of them being permanently stuck.
    #[test]
    fn no_two_buttons_share_a_bit() {
        let mut seen = 0_u32;
        for button in Button::ALL {
            assert_eq!(seen & button.bit(), 0, "{button:?} collides");
            seen |= button.bit();
        }
        assert_eq!(seen.count_ones() as usize, Button::ALL.len());
    }

    /// **A trigger's value and its button cannot disagree.**
    ///
    /// The failure this prevents is a pad reporting `L2` down at zero travel, which no
    /// hardware does and every title may assume cannot happen.
    #[test]
    fn a_trigger_and_its_button_move_together() {
        let mut pad = PadState::neutral();

        pad.set_trigger(false, 0.0);
        assert!(!pad.is_down(Button::L2));

        pad.set_trigger(false, TRIGGER_THRESHOLD);
        assert!(pad.is_down(Button::L2), "at the threshold it is down");
        assert!((pad.triggers[0] - TRIGGER_THRESHOLD).abs() < f32::EPSILON);

        pad.set_trigger(false, 0.1);
        assert!(!pad.is_down(Button::L2), "and it comes back up");
        assert!(
            !pad.is_down(Button::R2),
            "the other trigger was never touched"
        );
    }

    /// A travel outside the range is clamped rather than trusted.
    #[test]
    fn a_trigger_out_of_range_is_clamped() {
        let mut pad = PadState::neutral();
        pad.set_trigger(true, 9.0);
        assert!((pad.triggers[1] - 1.0).abs() < f32::EPSILON);
        pad.set_trigger(true, -3.0);
        assert!((pad.triggers[1] - 0.0).abs() < f32::EPSILON);
    }

    /// **A title never sees the shell button, and sees everything else.**
    ///
    /// The property the whole focus arrangement rests on. Asserted on both halves: hiding
    /// the shell button is useless if it also hides the press somebody made at the same
    /// time, and a title that lost half its input when somebody reached for the shell
    /// would be worse off than one that saw the button.
    #[test]
    fn a_title_sees_every_button_except_the_shells() {
        let mut pad = PadState::neutral();
        pad.set(Button::Shell, true);
        pad.set(Button::South, true);

        let seen = pad.as_title_sees_it();
        assert!(!seen.is_down(Button::Shell));
        assert!(seen.is_down(Button::South));
        assert!(
            pad.is_down(Button::Shell),
            "and the shell's own view is untouched"
        );
    }
}
