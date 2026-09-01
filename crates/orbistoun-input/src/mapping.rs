//! How many pads there are, and what drives each one.
//!
//! # Why keys are strings here
//!
//! This crate names a key as text and never as a windowing library's enum. Principle 12
//! says abstract at the level of what the guest asks for rather than what the host
//! provides, and a key is about as host-shaped as anything gets. A mapping written against
//! one window toolkit would carry that toolkit into the input contract, and then into the
//! settings file, where it would outlive any decision to change toolkits.
//!
//! So the window resolves a name to whatever its own key type is, and a name it does not
//! recognise is reported rather than dropped: a mapping that silently ignores a typo is a
//! button that does nothing for a reason nobody can see.
//!
//! # Why a port can be empty
//!
//! Two pads configured and one controller plugged in is the ordinary case, not an error.
//! An empty port is a pad a title can see and nobody is holding, which is a real state - and
//! a much better answer than pretending the port is not there, because a title that
//! enumerates pads at startup would then never find the one somebody plugs in later.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::pad::Button;

/// Most pads this emulator offers.
///
/// **Ours rather than the hardware's.** Nothing here has measured how many the target
/// permits; four is what this offers, and a title that asks for a fifth is told there is
/// not one, which is a state any title supporting fewer players already handles.
pub const MAX_PORTS: usize = 4;

/// What drives one port.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum Source {
    /// The port exists and nothing is holding it.
    #[default]
    Empty,
    /// Driven by this port's key mapping.
    Keyboard,
    /// Driven by the nth gamepad the host reports.
    ///
    /// By index rather than by name, because a name is not stable across a replug on every
    /// platform and an index at least fails in an obvious way - the wrong pad moves, which
    /// somebody notices immediately.
    Gamepad {
        /// Which host gamepad, counting from zero.
        index: usize,
    },
}

/// One way a key can push a stick.
///
/// **A direction rather than an axis**, because a key is on or off and an axis is a
/// number: one key per axis could only ever move it one way. Naming the eight pushes makes
/// a keyboard mapping say exactly what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Push {
    /// Left stick, upward.
    LeftUp,
    /// Left stick, downward.
    LeftDown,
    /// Left stick, to the left.
    LeftLeft,
    /// Left stick, to the right.
    LeftRight,
    /// Right stick, upward.
    RightUp,
    /// Right stick, downward.
    RightDown,
    /// Right stick, to the left.
    RightLeft,
    /// Right stick, to the right.
    RightRight,
}

impl Push {
    /// Every push, for iterating a mapping.
    pub const ALL: [Self; 8] = [
        Self::LeftUp,
        Self::LeftDown,
        Self::LeftLeft,
        Self::LeftRight,
        Self::RightUp,
        Self::RightDown,
        Self::RightLeft,
        Self::RightRight,
    ];

    /// The name used in a mapping file and in the settings window.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::LeftUp => "left stick up",
            Self::LeftDown => "left stick down",
            Self::LeftLeft => "left stick left",
            Self::LeftRight => "left stick right",
            Self::RightUp => "right stick up",
            Self::RightDown => "right stick down",
            Self::RightLeft => "right stick left",
            Self::RightRight => "right stick right",
        }
    }

    /// Which stick this pushes, and how far along which axis.
    ///
    /// The `y` axis is positive downward, matching [`crate::pad::Stick`] and every
    /// windowing system on this host - so a source never has to flip one axis and leave
    /// somebody wondering later which one was flipped.
    #[must_use]
    pub fn amount(self) -> (usize, f32, f32) {
        match self {
            Self::LeftUp => (0, 0.0, -1.0),
            Self::LeftDown => (0, 0.0, 1.0),
            Self::LeftLeft => (0, -1.0, 0.0),
            Self::LeftRight => (0, 1.0, 0.0),
            Self::RightUp => (1, 0.0, -1.0),
            Self::RightDown => (1, 0.0, 1.0),
            Self::RightLeft => (1, -1.0, 0.0),
            Self::RightRight => (1, 1.0, 0.0),
        }
    }
}

/// One port's configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Port {
    /// What drives it.
    pub source: Source,
    /// Which key stands for which button, when the source is the keyboard.
    ///
    /// Kept even when the source is not the keyboard, so switching a port to keyboard and
    /// back does not throw away a mapping somebody spent time on.
    pub keys: BTreeMap<Button, String>,
    /// Which key pushes which stick, which way.
    ///
    /// **Separate from `keys` because a stick is not a button.** A button is a bit and an
    /// axis is a number, and a mapping that tried to hold both in one table would have to
    /// pick one shape and lie about the other.
    #[serde(default)]
    pub axes: BTreeMap<Push, String>,
}

impl Default for Port {
    fn default() -> Self {
        Self {
            source: Source::Empty,
            keys: BTreeMap::new(),
            axes: BTreeMap::new(),
        }
    }
}

/// The default keyboard layout for the first port.
///
/// Chosen so somebody can reach the shell and move around without reading anything: arrows
/// for the pad, the home row for the face buttons, and a key of its own for the shell
/// button - which is the one this whole subsystem exists to make pressable.
#[must_use]
pub fn default_keys() -> BTreeMap<Button, String> {
    [
        (Button::Up, "ArrowUp"),
        (Button::Down, "ArrowDown"),
        (Button::Left, "ArrowLeft"),
        (Button::Right, "ArrowRight"),
        (Button::South, "K"),
        (Button::East, "L"),
        (Button::West, "J"),
        (Button::North, "I"),
        (Button::L1, "Q"),
        (Button::R1, "E"),
        (Button::L2, "1"),
        (Button::R2, "3"),
        (Button::L3, "Z"),
        (Button::R3, "C"),
        (Button::Select, "Backspace"),
        (Button::Start, "Enter"),
        (Button::Shell, "Home"),
    ]
    .into_iter()
    .map(|(button, key)| (button, key.to_owned()))
    .collect()
}

/// The default stick layout for the first port.
///
/// **Bound out of the box, because the default port is a keyboard and most titles are 3D.**
/// A shipped configuration that can press seventeen buttons and move neither stick cannot
/// play them, and nothing would have said so.
///
/// The left stick takes the usual four; the right takes the block beside them, which leaves
/// the arrows free for the directional pad - and the pad is what moves the shell, so it has
/// to stay somewhere obvious.
#[must_use]
pub fn default_axes() -> BTreeMap<Push, String> {
    [
        (Push::LeftUp, "W"),
        (Push::LeftDown, "S"),
        (Push::LeftLeft, "A"),
        (Push::LeftRight, "D"),
        (Push::RightUp, "T"),
        (Push::RightDown, "G"),
        (Push::RightLeft, "F"),
        (Push::RightRight, "H"),
    ]
    .into_iter()
    .map(|(push, key)| (push, key.to_owned()))
    .collect()
}

/// Every port, and what drives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pads {
    /// One entry per port. Its length is the controller count.
    pub ports: Vec<Port>,
}

impl Default for Pads {
    fn default() -> Self {
        // One port, driven by the keyboard. A default of zero would mean a fresh
        // installation has no way to press anything, and a default of four would put three
        // empty pads in front of somebody who has one keyboard.
        Self {
            ports: vec![Port {
                source: Source::Keyboard,
                keys: default_keys(),
                axes: default_axes(),
            }],
        }
    }
}

impl Pads {
    /// How many ports there are.
    #[must_use]
    pub fn count(&self) -> usize {
        self.ports.len()
    }

    /// Changes the number of ports, keeping what is already configured.
    ///
    /// **Growing adds empty ports and shrinking drops the last ones**, rather than
    /// rebuilding the list. Somebody who set up port two and then changed the count to
    /// three has not asked for port two to be forgotten.
    ///
    /// Clamped to `1..=MAX_PORTS`: zero ports is a machine nobody can press anything on,
    /// which is never what a person meant to ask for.
    pub fn set_count(&mut self, count: usize) {
        let count = count.clamp(1, MAX_PORTS);
        while self.ports.len() < count {
            self.ports.push(Port::default());
        }
        self.ports.truncate(count);
    }

    /// Keys bound to more than one button on one port.
    ///
    /// **Reported rather than resolved.** One key that presses two buttons is almost always
    /// a mistake made while rebinding, and the alternative - letting whichever came first
    /// win - is a binding that half works with nothing saying so.
    #[must_use]
    pub fn conflicts(&self) -> Vec<Conflict> {
        // **One map across every port, not one per port.** Built inside the loop, a key
        // bound on two ports was never reported - and the two ways to set up a second
        // keyboard player were to copy this port's layout, which silently collides on all
        // seventeen and makes one person drive two pads, or to bind them all by hand. The
        // docstring's own argument applies with a wider blast radius: a binding that half
        // works with nothing saying so.
        let mut seen: BTreeMap<&str, (usize, String)> = BTreeMap::new();
        let mut found = Vec::new();
        for (port, held) in self.ports.iter().enumerate() {
            let bindings = held
                .keys
                .iter()
                .map(|(button, key)| (key, button.label().to_owned()))
                .chain(
                    held.axes
                        .iter()
                        .map(|(push, key)| (key, push.label().to_owned())),
                );
            for (key, what) in bindings {
                if let Some((already_port, already)) = seen.get(key.as_str()) {
                    found.push(Conflict {
                        port,
                        other_port: *already_port,
                        key: key.clone(),
                        bound: [already.clone(), what],
                    });
                } else {
                    seen.insert(key.as_str(), (port, what));
                }
            }
        }
        found
    }
}

/// One key bound to two things, on one port or across two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// Where the second binding is.
    pub port: usize,
    /// Where the first one was. The same as `port` when the clash is within one.
    pub other_port: usize,
    /// The key bound twice.
    pub key: String,
    /// What it is bound to, first and second - a button or a stick push.
    pub bound: [String; 2],
}

impl Conflict {
    /// Whether the clash is between two different ports.
    ///
    /// Worth distinguishing: within a port it is usually a slip while rebinding, and across
    /// ports it means two players would move together.
    #[must_use]
    pub fn across_ports(&self) -> bool {
        self.port != self.other_port
    }

    /// One line a person reads.
    #[must_use]
    pub fn say(&self) -> String {
        if self.across_ports() {
            return format!(
                "{} is bound on port {} ({}) and port {} ({}) - both would move together",
                self.key,
                self.other_port + 1,
                self.bound[0],
                self.port + 1,
                self.bound[1]
            );
        }
        format!(
            "port {}: {} is bound to both {} and {}",
            self.port + 1,
            self.key,
            self.bound[0],
            self.bound[1]
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{MAX_PORTS, Pads, Port, Source, default_keys};
    use crate::pad::Button;

    /// **A fresh installation can press something.**
    ///
    /// A default of no ports, or of ports with no source, is a machine that looks broken to
    /// somebody who has not opened the settings window yet.
    #[test]
    fn the_default_is_one_port_somebody_can_actually_use() {
        let pads = Pads::default();
        assert_eq!(pads.count(), 1);
        assert_eq!(pads.ports[0].source, Source::Keyboard);
        assert!(
            pads.ports[0].keys.contains_key(&Button::Shell),
            "the shell button is the one this exists to make pressable"
        );
    }

    /// Every button has a default key, so nothing is unreachable out of the box.
    #[test]
    fn the_default_layout_binds_every_button() {
        let keys = default_keys();
        for button in Button::ALL {
            assert!(keys.contains_key(&button), "{button:?} has no default key");
        }
    }

    /// **Changing the count keeps what was already set up.**
    #[test]
    fn growing_and_shrinking_preserves_configured_ports() {
        let mut pads = Pads::default();
        pads.set_count(3);
        pads.ports[1].source = Source::Gamepad { index: 0 };

        pads.set_count(4);
        assert_eq!(
            pads.ports[1].source,
            Source::Gamepad { index: 0 },
            "port two was not asked to be forgotten"
        );

        pads.set_count(2);
        assert_eq!(pads.count(), 2);
        assert_eq!(pads.ports[1].source, Source::Gamepad { index: 0 });
    }

    /// A count outside the range is clamped, never taken literally.
    #[test]
    fn a_count_of_zero_or_a_hundred_is_refused_quietly() {
        let mut pads = Pads::default();
        pads.set_count(0);
        assert_eq!(pads.count(), 1, "nobody meant to have no controllers");
        pads.set_count(99);
        assert_eq!(pads.count(), MAX_PORTS);
    }

    /// **A key bound twice is reported, not silently resolved.**
    ///
    /// Asserted on the failure: letting the first binding win produces a rebind that half
    /// worked, with nothing anywhere saying why.
    #[test]
    fn one_key_on_two_buttons_is_a_reported_conflict() {
        let mut pads = Pads::default();
        pads.ports[0].keys.insert(Button::North, "K".to_owned());

        let conflicts = pads.conflicts();
        assert_eq!(conflicts.len(), 1, "{conflicts:?}");
        assert!(conflicts[0].say().contains('K'), "{}", conflicts[0].say());
        assert!(conflicts[0].say().contains("port 1"));
    }

    /// The shipped default has no conflicts in it.
    #[test]
    fn the_default_layout_is_not_self_contradictory() {
        assert!(Pads::default().conflicts().is_empty());
    }

    /// Configuration survives a round trip, including an empty port.
    #[test]
    fn a_configuration_survives_being_written_and_read() {
        let mut pads = Pads::default();
        pads.set_count(2);
        pads.ports[1] = Port {
            source: Source::Gamepad { index: 1 },
            keys: BTreeMap::default(),
            axes: BTreeMap::default(),
        };

        let text = toml::to_string_pretty(&pads).expect("serialises");
        let back: Pads = toml::from_str(&text).expect("parses");
        assert_eq!(back, pads);
    }

    /// **A key bound on two ports is a conflict too, and that was the whole bug.**
    ///
    /// The check built its seen-map *inside* the per-port loop, so a clash across ports was
    /// invisible. That left exactly two ways to add a second keyboard player: copy this
    /// port's layout and have all seventeen keys silently drive both pads, or bind every one
    /// by hand. Found by review against a parallel implementation (D341).
    #[test]
    fn one_key_on_two_ports_is_a_reported_conflict() {
        let mut pads = Pads::default();
        pads.set_count(2);
        pads.ports[1].source = Source::Keyboard;
        pads.ports[1].keys = default_keys();

        let conflicts = pads.conflicts();
        assert!(
            !conflicts.is_empty(),
            "a copied layout collides on every key"
        );
        assert!(
            conflicts.iter().all(super::Conflict::across_ports),
            "and every one of them is across ports"
        );
        assert!(
            conflicts[0].say().contains("move together"),
            "{}",
            conflicts[0].say()
        );
    }

    /// **Opposite pushes cancel to centre rather than one winning.**
    ///
    /// A keyboard can hold left and right at once and a stick cannot be in two places. The
    /// reader sums and clamps, so the pair means "not pushed" - a position a stick can
    /// actually be in. Letting the first one win would make left+right mean something no pad
    /// can express (D341).
    #[test]
    fn opposite_pushes_sum_to_centre() {
        for (a, b) in [
            (super::Push::LeftLeft, super::Push::LeftRight),
            (super::Push::LeftUp, super::Push::LeftDown),
            (super::Push::RightLeft, super::Push::RightRight),
            (super::Push::RightUp, super::Push::RightDown),
        ] {
            let (stick, ax, ay) = a.amount();
            let (other, bx, by) = b.amount();
            assert_eq!(stick, other, "{a:?} and {b:?} are the same stick");
            assert!((ax + bx).abs() < f32::EPSILON, "{a:?} + {b:?} on x");
            assert!((ay + by).abs() < f32::EPSILON, "{a:?} + {b:?} on y");
        }
    }

    /// **The shipped layout can move a stick.**
    ///
    /// The default port is a keyboard, so a default with no stick bindings is a shipped
    /// configuration that cannot play most 3D titles - and nothing would have said so.
    #[test]
    fn the_default_layout_binds_both_sticks() {
        let pads = Pads::default();
        for push in super::Push::ALL {
            assert!(
                pads.ports[0].axes.contains_key(&push),
                "{push:?} has no default key"
            );
        }
    }
}
