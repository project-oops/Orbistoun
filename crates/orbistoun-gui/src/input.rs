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
    /// The host keys held that no pad port binds, as USB HID usages (D783).
    pub(crate) keys: Vec<u16>,
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

        let keys = ctx.input(|input| unbound_usages(&held(input), pads));
        Frame {
            keys,
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

/// A key's USB HID usage code, as a keyboard reports it (USB HID Usage Tables, keyboard page 0x07),
/// or `None` for a key a keyboard record has no code for here.
fn usage(key: egui::Key) -> Option<u16> {
    Some(match key {
        egui::Key::A => 0x04,
        egui::Key::B => 0x05,
        egui::Key::C => 0x06,
        egui::Key::D => 0x07,
        egui::Key::E => 0x08,
        egui::Key::F => 0x09,
        egui::Key::G => 0x0a,
        egui::Key::H => 0x0b,
        egui::Key::I => 0x0c,
        egui::Key::J => 0x0d,
        egui::Key::K => 0x0e,
        egui::Key::L => 0x0f,
        egui::Key::M => 0x10,
        egui::Key::N => 0x11,
        egui::Key::O => 0x12,
        egui::Key::P => 0x13,
        egui::Key::Q => 0x14,
        egui::Key::R => 0x15,
        egui::Key::S => 0x16,
        egui::Key::T => 0x17,
        egui::Key::U => 0x18,
        egui::Key::V => 0x19,
        egui::Key::W => 0x1a,
        egui::Key::X => 0x1b,
        egui::Key::Y => 0x1c,
        egui::Key::Z => 0x1d,
        egui::Key::Num1 => 0x1e,
        egui::Key::Num2 => 0x1f,
        egui::Key::Num3 => 0x20,
        egui::Key::Num4 => 0x21,
        egui::Key::Num5 => 0x22,
        egui::Key::Num6 => 0x23,
        egui::Key::Num7 => 0x24,
        egui::Key::Num8 => 0x25,
        egui::Key::Num9 => 0x26,
        egui::Key::Num0 => 0x27,
        egui::Key::Enter => 0x28,
        egui::Key::Escape => 0x29,
        egui::Key::Backspace => 0x2a,
        egui::Key::Tab => 0x2b,
        egui::Key::Space => 0x2c,
        egui::Key::Minus => 0x2d,
        egui::Key::Equals => 0x2e,
        egui::Key::OpenBracket => 0x2f,
        egui::Key::CloseBracket => 0x30,
        egui::Key::Backslash => 0x31,
        egui::Key::Semicolon => 0x33,
        egui::Key::Backtick => 0x35,
        egui::Key::Comma => 0x36,
        egui::Key::Period => 0x37,
        egui::Key::Slash => 0x38,
        egui::Key::F1 => 0x3a,
        egui::Key::F2 => 0x3b,
        egui::Key::F3 => 0x3c,
        egui::Key::F4 => 0x3d,
        egui::Key::F5 => 0x3e,
        egui::Key::F6 => 0x3f,
        egui::Key::F7 => 0x40,
        egui::Key::F8 => 0x41,
        egui::Key::F9 => 0x42,
        egui::Key::F10 => 0x43,
        egui::Key::F11 => 0x44,
        egui::Key::F12 => 0x45,
        egui::Key::Insert => 0x49,
        egui::Key::Home => 0x4a,
        egui::Key::PageUp => 0x4b,
        egui::Key::Delete => 0x4c,
        egui::Key::End => 0x4d,
        egui::Key::PageDown => 0x4e,
        egui::Key::ArrowRight => 0x4f,
        egui::Key::ArrowLeft => 0x50,
        egui::Key::ArrowDown => 0x51,
        egui::Key::ArrowUp => 0x52,
        _ => return None,
    })
}

/// The usages of the held keys no keyboard-driven port binds: a key bound to a pad button or
/// stick is the pad's, so one press never reaches a title twice (D783).
fn unbound_usages(held: &[egui::Key], pads: &Pads) -> Vec<u16> {
    let bound: Vec<egui::Key> = pads
        .ports
        .iter()
        .filter(|port| matches!(port.source, Source::Keyboard))
        .flat_map(|port| port.keys.values().chain(port.axes.values()))
        .filter_map(|name| egui::Key::from_name(name))
        .collect();
    held.iter()
        .filter(|key| !bound.contains(key))
        .filter_map(|&key| usage(key))
        .take(orbistoun_input::keyboard::MOST_KEYS)
        .collect()
}

/// The keys held this frame, in egui's order.
fn held(input: &egui::InputState) -> Vec<egui::Key> {
    egui::Key::ALL
        .iter()
        .copied()
        .filter(|&key| input.key_down(key))
        .collect()
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

    /// A held key reaches the keyboard as its USB HID usage, unless a keyboard port binds it to the
    /// pad, so one press never reaches a title twice (D783).
    #[test]
    fn held_keys_reach_the_keyboard_unless_a_pad_port_binds_them() {
        assert_eq!(super::usage(egui::Key::A), Some(0x04));
        assert_eq!(super::usage(egui::Key::Num1), Some(0x1e));
        assert_eq!(super::usage(egui::Key::Num0), Some(0x27));
        assert_eq!(super::usage(egui::Key::Enter), Some(0x28));
        assert_eq!(super::usage(egui::Key::ArrowUp), Some(0x52));
        assert_eq!(super::usage(egui::Key::F12), Some(0x45));

        let mut pads = Pads::default();
        pads.ports[0].keys.insert(Button::South, "Q".to_owned());
        let held = [egui::Key::Q, egui::Key::A, egui::Key::Space];
        let keys = super::unbound_usages(&held, &pads);
        assert!(!keys.contains(&0x14), "Q is the pad's: {keys:?}");
        assert!(keys.contains(&0x2c), "space is the keyboard's: {keys:?}");
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
