//! Telling a tap on the shell button from a hold.
//!
//! Pure: it takes elapsed milliseconds rather than reading a clock, so every edge is a test.
//! A hold fires while the button is still down, as hardware that does this behaves, so the
//! person holding it gets feedback before letting go. A release after a hold reports nothing,
//! so one press does not open two menus.

/// How long the shell button must be held to mean the second thing.
///
/// A choice, not a measurement: long enough that a hurried tap cannot reach it, short enough
/// that a deliberate hold does not feel unresponsive.
pub const HOLD_MS: u32 = 600;

/// What a press turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPress {
    /// Nothing has happened this frame.
    None,
    /// Pressed and released quickly.
    Tap,
    /// Held past [`HOLD_MS`], reported the moment it got there.
    Hold,
}

/// Watches the shell button across frames.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShellButton {
    /// How long it has been down, or zero when it is up.
    held_ms: u32,
    /// Whether this press has already been reported as a hold, so a hold fires once rather than
    /// every frame past the threshold.
    fired: bool,
}

impl ShellButton {
    /// Advances by one frame and says what happened.
    ///
    /// `elapsed_ms` is the time since the last call, passed in so the behaviour is testable
    /// without waiting.
    pub fn update(&mut self, down: bool, elapsed_ms: u32) -> ShellPress {
        if down {
            self.held_ms = self.held_ms.saturating_add(elapsed_ms);
            if !self.fired && self.held_ms >= HOLD_MS {
                self.fired = true;
                return ShellPress::Hold;
            }
            return ShellPress::None;
        }

        // Released. A press that never reached the threshold was a tap; one that did has already
        // been reported.
        let was_down = self.held_ms > 0;
        let tapped = was_down && !self.fired;
        self.held_ms = 0;
        self.fired = false;
        if tapped {
            ShellPress::Tap
        } else {
            ShellPress::None
        }
    }

    /// How far through a hold this press is, in `0.0..=1.0`, for drawing progress while the
    /// button is held.
    #[must_use]
    pub fn hold_progress(&self) -> f32 {
        if self.fired {
            return 1.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "both are small millisecond counts; the ratio is for drawing a bar"
        )]
        let progress = self.held_ms as f32 / HOLD_MS as f32;
        progress.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{HOLD_MS, ShellButton, ShellPress};

    /// A quick press and release is a tap.
    #[test]
    fn a_short_press_is_a_tap_on_release() {
        let mut button = ShellButton::default();
        assert_eq!(button.update(true, 50), ShellPress::None, "nothing yet");
        assert_eq!(button.update(false, 16), ShellPress::Tap);
    }

    /// A hold fires while the button is still down, not on release.
    #[test]
    fn a_long_press_reports_the_hold_before_it_is_released() {
        let mut button = ShellButton::default();
        assert_eq!(button.update(true, HOLD_MS - 1), ShellPress::None);
        assert_eq!(button.update(true, 1), ShellPress::Hold, "at the threshold");
    }

    /// A hold is reported once, however long the button stays down.
    #[test]
    fn a_hold_is_reported_once_and_not_every_frame_after() {
        let mut button = ShellButton::default();
        assert_eq!(button.update(true, HOLD_MS), ShellPress::Hold);
        for _ in 0..60 {
            assert_eq!(button.update(true, 16), ShellPress::None);
        }
    }

    /// Releasing after a hold is not also a tap.
    #[test]
    fn releasing_after_a_hold_reports_nothing() {
        let mut button = ShellButton::default();
        assert_eq!(button.update(true, HOLD_MS), ShellPress::Hold);
        assert_eq!(button.update(false, 16), ShellPress::None);
    }

    /// A button nobody touched reports nothing, however many frames pass.
    #[test]
    fn an_untouched_button_never_reports_anything() {
        let mut button = ShellButton::default();
        for _ in 0..100 {
            assert_eq!(button.update(false, 16), ShellPress::None);
        }
    }

    /// The press after a hold behaves like a fresh press.
    #[test]
    fn a_press_after_a_hold_can_still_be_a_tap() {
        let mut button = ShellButton::default();
        button.update(true, HOLD_MS);
        button.update(false, 16);

        assert_eq!(button.update(true, 20), ShellPress::None);
        assert_eq!(button.update(false, 16), ShellPress::Tap);
    }

    /// Progress runs from nothing to full and stops there.
    #[test]
    fn hold_progress_is_drawable_throughout() {
        let mut button = ShellButton::default();
        assert!((button.hold_progress() - 0.0).abs() < f32::EPSILON);

        button.update(true, HOLD_MS / 2);
        let half = button.hold_progress();
        assert!((0.4..=0.6).contains(&half), "{half}");

        button.update(true, HOLD_MS);
        assert!((button.hold_progress() - 1.0).abs() < f32::EPSILON);
    }
}
