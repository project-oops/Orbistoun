//! What a controller is, in our own terms.
//!
//! Buttons are named by position, never by glyph: vendor symbols stay out of this tree, a
//! keyboard mapping can only target a position, and a host gamepad reports positions too.
//! [`Button::South`] is the lower face button on every pad. The state is host-shaped (floats in
//! settled ranges) and describes what a person's hands are doing; conversion to the layout a
//! title reads is [`record`]'s job.

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
    /// Every other button is the title's while it has focus; this one is always the shell's,
    /// because it is how a person reaches the shell from a title. A title never sees it (D326).
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
/// Both axes in `-1.0..=1.0`, positive right and positive down, the convention every windowing
/// system on this host uses, so no source flips an axis.
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
    /// Held apart from `L2`/`R2`: a title reading the analogue value needs more than a
    /// past-the-threshold bit.
    pub triggers: [f32; 2],
}

/// How far a trigger travels before it counts as its button being down.
///
/// A chosen value, not measured; being wrong makes the button engage slightly early or late.
pub const TRIGGER_THRESHOLD: f32 = 0.5;

impl PadState {
    /// Nothing pressed, sticks centred.
    ///
    /// What a title is handed while the shell has focus: a quiet pad, not "no controller", which
    /// makes many titles stop or prompt.
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
    /// One call, so the analogue value and the bit cannot disagree (no pad reports `L2` down at
    /// zero travel).
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

    /// The same state with the shell's own button removed: what a title is allowed to see, so it
    /// never acts on a press meant for the shell (D326).
    #[must_use]
    pub fn as_title_sees_it(&self) -> Self {
        let mut seen = *self;
        seen.set(Button::Shell, false);
        seen
    }
}

/// How many bytes a pad read writes.
///
/// obSCEne's `100-input/read-extent` fills a buffer with a sentinel, calls `scePadReadState` and
/// reports `extent 120`, `changed 120`; `100-input/batched-read` reports the same for
/// `scePadRead`. The structure is 120 bytes and the call writes all of them.
pub const STATE_BYTES: usize = 120;

/// The 120 bytes the hardware wrote for a pad at rest (obSCEne `100-input/read-extent` and
/// `100-input/batched-read`, title leg).
///
/// A byte image rather than a `struct`: which offset carries which field is an inference from
/// it, not a measurement. [`record`] places input over it at the positions the collection's
/// SDK reads on hardware (D713); every other byte keeps its measured value.
pub const AT_REST: &[u8; STATE_BYTES] = &[
    // 0: 00000000 80808080 00000000 00000000
    0x00, 0x00, 0x00, 0x00, 0x80, 0x80, 0x80, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 16: 00000000 00000000 0000803f 00000000 (0x3f800000 is 1.0f little-endian)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00,
    // 32: 0000803f 00000000 00000000 00000000
    0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 48..120: zero, every row
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Where each button's bit is in the record's button word (D713).
///
/// The SDK's bits (`oops-sdk/include/oops/input.h`). `Select` as the touchpad has not been
/// exercised. `Shell` has no bit, since a title never sees it.
const fn record_bit(button: Button) -> u32 {
    match button {
        Button::L3 => 1 << 1,
        Button::R3 => 1 << 2,
        Button::Start => 1 << 3,
        Button::Up => 1 << 4,
        Button::Right => 1 << 5,
        Button::Down => 1 << 6,
        Button::Left => 1 << 7,
        Button::L2 => 1 << 8,
        Button::R2 => 1 << 9,
        Button::L1 => 1 << 10,
        Button::R1 => 1 << 11,
        Button::North => 1 << 12,
        Button::East => 1 << 13,
        Button::South => 1 << 14,
        Button::West => 1 << 15,
        Button::Select => 1 << 20,
        Button::Shell => 0,
    }
}

/// Every button, for walking the state into the word.
const BUTTONS: [Button; 17] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::L1,
    Button::R1,
    Button::L2,
    Button::R2,
    Button::L3,
    Button::R3,
    Button::Up,
    Button::Down,
    Button::Left,
    Button::Right,
    Button::Select,
    Button::Start,
    Button::Shell,
];

/// An axis in `-1.0..=1.0` as the record's byte: 0 at one extreme, 0x80 at centre, 0xFF at the
/// other.
fn axis_byte(value: f32) -> u8 {
    let scaled = (value.clamp(-1.0, 1.0) + 1.0) * 127.5;
    // In 0..=255 by the clamp, so the cast is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = scaled.round().min(255.0) as u8;
    byte
}

/// A pad's state as the 120-byte record a read writes (D713): the measured at-rest image, with
/// the fields the SDK places - buttons at 0, sticks at 4 and 6, triggers at 8, connected at 76 -
/// written from `state`.
#[must_use]
pub fn record(state: &PadState) -> [u8; STATE_BYTES] {
    let mut bytes = *AT_REST;
    let word = BUTTONS
        .iter()
        .filter(|&&b| state.is_down(b))
        .fold(0_u32, |word, &b| word | record_bit(b));
    bytes[0..4].copy_from_slice(&word.to_le_bytes());
    bytes[4] = axis_byte(state.sticks[0].x);
    bytes[5] = axis_byte(state.sticks[0].y);
    bytes[6] = axis_byte(state.sticks[1].x);
    bytes[7] = axis_byte(state.sticks[1].y);
    // A trigger's 0..=1 onto the stick scale's upper half and past it: 0 at rest, 0xFF held.
    bytes[8] = axis_byte(state.triggers[0] * 2.0 - 1.0);
    bytes[9] = axis_byte(state.triggers[1] * 2.0 - 1.0);
    bytes[76] = 1;
    bytes
}

#[cfg(test)]
mod tests {
    use super::{Button, PadState, TRIGGER_THRESHOLD};

    /// A quiet pad is the at-rest image with only the connected flag changed, and a press lands on
    /// the SDK's bit (cross is bit 14, d-pad down bit 6).
    #[test]
    fn a_record_places_the_state_where_the_sdk_reads_it() {
        let quiet = super::record(&PadState::neutral());
        let mut expected = *super::AT_REST;
        expected[76] = 1;
        assert_eq!(quiet, expected);

        let mut pad = PadState::neutral();
        pad.set(Button::South, true);
        pad.set(Button::Down, true);
        pad.set(Button::Shell, true);
        pad.sticks[0].x = -1.0;
        pad.triggers[1] = 1.0;
        let bytes = super::record(&pad);
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            (1 << 14) | (1 << 6),
            "cross and down, and never the shell's own button"
        );
        assert_eq!((bytes[4], bytes[5], bytes[9]), (0x00, 0x80, 0xFF));
    }

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

    /// Every button has its own bit; a variant inserted mid-enum renumbers the rest.
    #[test]
    fn no_two_buttons_share_a_bit() {
        let mut seen = 0_u32;
        for button in Button::ALL {
            assert_eq!(seen & button.bit(), 0, "{button:?} collides");
            seen |= button.bit();
        }
        assert_eq!(seen.count_ones() as usize, Button::ALL.len());
    }

    /// A trigger's value and its button move together.
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

    /// A title sees every button except the shell's, including one pressed at the same time.
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
