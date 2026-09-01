//! Reading real input, and deciding who is allowed to see it.
//!
//! # Why the window owns this and the worker does not
//!
//! The guest runs in a child process (D032) and the keyboard belongs to whichever window
//! has focus, which is this one. That is not an inconvenience - it is what makes the shell
//! button work at all. **The system's own button has to be seen by something that is not
//! the title**, and a worker reading input directly would have no way to keep one button
//! back.
//!
//! So input is read here, the shell takes its button, and what is left is what a title may
//! see. The second half is not yet carried across the process boundary, and deliberately:
//! nothing on the far side can consume it, because the structure a title reads a pad into
//! is unmeasured (D326). Building the transport before there is anything to receive it is
//! speculation by principle 12's own test.
//!
//! # What is missing, plainly
//!
//! Real gamepads. [`orbistoun_input::Source::Gamepad`] is in the configuration and nothing
//! here reads one, so a port set to it reports a neutral pad. That is a stated gap rather
//! than a silent one: the settings pane says so next to the control.

use orbistoun_input::{Button, PadState, Pads, ShellButton, ShellPress, Source};

/// Reads host input each frame and keeps what has to survive between frames.
#[derive(Debug, Default)]
pub(crate) struct Reader {
    /// The shell button's press, across frames.
    ///
    /// **One, not one per port.** The shell is a single thing and it opens once; four
    /// people holding four pads are not four shells. Any port may press it.
    shell: ShellButton,
    /// What was down last frame, per port.
    ///
    /// **Navigation needs edges, not levels.** A menu that moved once per frame while a
    /// direction was held would cross a library of twelve titles in a fifth of a second,
    /// which is not a menu anybody can aim.
    previous: Vec<u32>,
}

/// What one frame of input amounted to.
pub(crate) struct Frame {
    /// One state per configured port, in port order.
    ///
    /// Held even when nothing drives a port: an empty port is a pad nobody is holding,
    /// which is a real state a title may enumerate (see `orbistoun_input::mapping`).
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
    /// `elapsed_ms` comes from the caller rather than a clock in here, so the press-versus-
    /// hold decision stays the tested one in `orbistoun_input::shell_button`.
    pub(crate) fn read(&mut self, ctx: &egui::Context, pads: &Pads, elapsed_ms: u32) -> Frame {
        let states: Vec<PadState> = pads
            .ports
            .iter()
            .map(|port| match port.source {
                Source::Keyboard => ctx.input(|input| keyboard(input, port)),
                // A port with nothing driving it, and a port set to a gamepad this build
                // cannot read, are the same to a title: a pad nobody is touching. They are
                // not the same to a person, which is why the settings pane distinguishes
                // them and this does not have to.
                Source::Empty | Source::Gamepad { .. } => PadState::neutral(),
            })
            .collect();

        // Any port may reach the shell. Held rather than pressed, because the decision
        // between a tap and a hold is about duration and this only reports the level.
        let down = states.iter().any(|state| state.is_down(Button::Shell));
        let press = self.shell.update(down, elapsed_ms);

        // Buttons that went down this frame, across every port. Pooled rather than kept per
        // port because the shell is one thing being navigated: whoever presses a direction
        // moves the highlight, and four people fighting over it is their problem rather than
        // a case this has to model.
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
/// One direction per frame: two pressed together is somebody rolling a thumb across a
/// stick, and picking the first is steadier than trying to honour both.
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
        // A name egui does not recognise is skipped here and **reported by the settings
        // pane**, not swallowed. A typo that silently binds nothing is a button somebody
        // presses repeatedly while wondering what is broken.
        let Some(key) = egui::Key::from_name(name) else {
            continue;
        };
        // **Level or edge.** A key held across frames reports down, which is what a hold
        // needs - but a press and release that both land inside one frame would otherwise
        // be invisible, and the shortest real tap on a fast display is close to that. The
        // edge makes a one-frame press count without affecting anything held longer.
        if !input.key_down(key) && !input.key_pressed(key) {
            continue;
        }
        match button {
            // Through `set_trigger`, so the analogue value and the bit agree. A key is all
            // or nothing, so a held key is a fully pulled trigger.
            Button::L2 => state.set_trigger(false, 1.0),
            Button::R2 => state.set_trigger(true, 1.0),
            other => state.set(*other, true),
        }
    }

    // **Sticks, which a keyboard could not move at all until this existed.** The shipped
    // configuration is one keyboard port, so without it the out-of-the-box setup could
    // press every button and drive nothing analogue - which is most 3D titles.
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
    // **Opposite directions cancel to centre rather than one winning.** A keyboard can hold
    // left and right at once and a stick cannot be in two places, so summing and clamping
    // makes the pair mean "not pushed" - which is a position a stick can actually be in.
    // Letting the first one win would make left+right mean something no pad can express.
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

    /// **Every key in the shipped layout resolves.**
    ///
    /// The default mapping is written as text in a crate that has never heard of this
    /// window, so nothing but a test connects the two. A typo there is a button that does
    /// nothing on a fresh installation, with no error anywhere.
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
