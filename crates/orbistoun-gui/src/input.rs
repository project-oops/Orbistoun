//! Reads host input and decides who may see it.
//!
//! The window owns host input (D326): the guest runs in a child process and the keyboard
//! belongs to the focused window. The shell takes its own button here, and what is left is
//! what a title may see. A port set to [`orbistoun_input::Source::Gamepad`] is not read by
//! this window and reports a neutral pad; the settings pane says so beside the control.

use orbistoun_input::{Button, PadState, Pads, ShellButton, ShellPress, Source};

/// Reads host input each frame and keeps what has to survive between frames.
#[derive(Debug, Default)]
pub(crate) struct Reader {
    /// The shell button's press, across frames.
    ///
    /// One, not one per port: there is a single shell, and any port may press it.
    shell: ShellButton,
    /// What was down last frame, per port.
    ///
    /// Navigation needs edges, not levels: a held direction moves the highlight once.
    previous: Vec<u32>,
}

/// What one frame of input amounted to.
pub(crate) struct Frame {
    /// One state per configured port, in port order.
    ///
    /// Held even when nothing drives a port: an empty port is a pad nobody is holding, a
    /// state a title may enumerate (see `orbistoun_input::mapping`).
    pub(crate) pads: Vec<PadState>,
    /// What the shell button did.
    pub(crate) shell: ShellPress,
    /// Buttons that went down this frame, pooled across ports.
    pub(crate) just_pressed: u32,
    /// How far through a hold the shell button is, for drawing.
    pub(crate) hold_progress: f32,
}

impl Reader {
    /// Reads one frame.
    ///
    /// `elapsed_ms` comes from the caller rather than a clock here, so the press-or-hold
    /// decision stays the tested one in `orbistoun_input::shell_button`.
    pub(crate) fn read(&mut self, ctx: &egui::Context, pads: &Pads, elapsed_ms: u32) -> Frame {
        let states: Vec<PadState> = pads
            .ports
            .iter()
            .map(|port| match port.source {
                Source::Keyboard => ctx.input(|input| keyboard(input, port)),
                // None of these is fed by this live reader. A script is sampled in the worker
                // as a pure function of run time (`orbistoun_input::script`, D707).
                Source::Empty | Source::Gamepad { .. } | Source::Script { .. } => {
                    PadState::neutral()
                }
            })
            .collect();

        // Any port may reach the shell. The level is reported; tap-or-hold is decided by
        // duration downstream.
        let down = states.iter().any(|state| state.is_down(Button::Shell));
        let press = self.shell.update(down, elapsed_ms);

        // Buttons that went down this frame, pooled across ports because one shell is being
        // navigated.
        self.previous.resize(states.len(), 0);
        let mut edges = 0_u32;
        for (index, state) in states.iter().enumerate() {
            edges |= state.pressed() & !self.previous[index];
            self.previous[index] = state.pressed();
        }

        Frame {
            pads: states,
            shell: press,
            hold_progress: self.shell.hold_progress(),
            just_pressed: edges,
        }
    }
}

/// Which way a frame's fresh presses point, if any.
///
/// One direction per frame; with two pressed together the first wins.
#[must_use]
pub(crate) fn steering(just_pressed: u32) -> Option<orbistoun_shell::Move> {
    use orbistoun_shell::Move;

    [
        (Button::Left, Move::Left),
        (Button::Right, Move::Right),
        (Button::Up, Move::Up),
        (Button::Down, Move::Down),
    ]
    .into_iter()
    .find_map(|(button, direction)| (just_pressed & button.bit() != 0).then_some(direction))
}

/// One port's state, from the keyboard.
fn keyboard(input: &egui::InputState, port: &orbistoun_input::Port) -> PadState {
    let mut state = PadState::neutral();
    for (button, name) in &port.keys {
        // A name egui does not recognise is skipped here and reported by the settings pane.
        let Some(key) = egui::Key::from_name(name) else {
            continue;
        };
        // Level or edge: a held key reports down, and the edge makes a press and release
        // inside one frame still count.
        if !input.key_down(key) && !input.key_pressed(key) {
            continue;
        }
        match button {
            // Through `set_trigger`, so the analogue value and the bit agree; a held key is
            // a fully pulled trigger.
            Button::L2 => state.set_trigger(false, 1.0),
            Button::R2 => state.set_trigger(true, 1.0),
            other => state.set(*other, true),
        }
    }

    // Sticks from named key pushes (D341), so the default keyboard port drives analogue input.
    for (push, name) in &port.axes {
        let Some(key) = egui::Key::from_name(name) else {
            continue;
        };
        if !input.key_down(key) && !input.key_pressed(key) {
            continue;
        }
        let (stick, x, y) = push.amount();
        state.sticks[stick].x += x;
        state.sticks[stick].y += y;
    }
    // Opposite directions sum and cancel to centre, a position a real stick can hold.
    for stick in &mut state.sticks {
        stick.x = stick.x.clamp(-1.0, 1.0);
        stick.y = stick.y.clamp(-1.0, 1.0);
    }
    state
}

/// Key names in a mapping that this window cannot resolve.
///
/// Returned rather than logged, so the settings pane can put the problem beside the control
/// that caused it.
pub(crate) fn unresolved(pads: &Pads) -> Vec<String> {
    let mut bad = Vec::new();
    for (index, port) in pads.ports.iter().enumerate() {
        let named = port
            .keys
            .iter()
            .map(|(button, name)| (button.label().to_owned(), name))
            .chain(
                port.axes
                    .iter()
                    .map(|(push, name)| (push.label().to_owned(), name)),
            );
        for (what, name) in named {
            if egui::Key::from_name(name).is_none() {
                bad.push(format!(
                    "port {}: {what} is bound to \"{name}\", which is not a key name",
                    index + 1
                ));
            }
        }
    }
    bad
}

#[cfg(test)]
mod tests {
    use orbistoun_input::{Button, Pads};

    /// Every key name in the shipped default mapping resolves to an egui key.
    #[test]
    fn the_default_layout_names_keys_this_window_understands() {
        let unresolved = super::unresolved(&Pads::default());
        assert!(unresolved.is_empty(), "{unresolved:?}");
    }

    /// A name that is not a key is reported, with enough to find it.
    #[test]
    fn an_unresolvable_key_name_is_reported_rather_than_dropped() {
        let mut pads = Pads::default();
        pads.ports[0]
            .keys
            .insert(Button::South, "NotAKeyAtAll".to_owned());

        let unresolved = super::unresolved(&pads);
        assert_eq!(unresolved.len(), 1, "{unresolved:?}");
        assert!(unresolved[0].contains("south"), "{}", unresolved[0]);
        assert!(unresolved[0].contains("NotAKeyAtAll"), "{}", unresolved[0]);
    }
}
