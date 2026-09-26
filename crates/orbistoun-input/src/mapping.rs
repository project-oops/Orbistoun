//! How many pads there are, and what drives each one.
//!
//! Keys are named as text, never as a windowing library's enum, so no toolkit leaks into the
//! input contract or the settings file. The window resolves a name to its own key type and
//! reports a name it does not recognise rather than dropping it. A port can be empty: a pad a
//! title can see and nobody is holding is a real state, and a title enumerating pads at
//! startup still finds it when a controller is plugged in later.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::pad::Button;

/// Most pads this emulator offers.
///
/// This emulator's choice, not a measured hardware limit; a title asking for a fifth is told
/// there is none, as a title supporting fewer players already handles.
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
    /// By index rather than name: names are not stable across a replug on every platform, and a
    /// wrong index shows at once as the wrong pad moving.
    Gamepad {
        /// Which host gamepad, counting from zero.
        index: usize,
    },
    /// Driven by a recorded script of timed pad states, read from a file.
    ///
    /// The deterministic source, and the only one a headless worker can drive: live keyboard and
    /// gamepad input reaches a run only through the GUI shim. A script is a pure function of run
    /// time ([`crate::script`]), so the same file and guest give the same run (D707). The worker
    /// reads and validates the file, and a path or script that fails fails the run.
    Script {
        /// Where the script file is, resolved by the reader relative to the configuration.
        path: String,
    },
}

/// One way a key can push a stick.
///
/// A direction rather than an axis: a key is on or off, so one key per axis could move it only
/// one way.
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
    /// `y` is positive downward, matching [`crate::pad::Stick`] and every windowing system on this
    /// host.
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
    /// Kept when the source changes, so switching a port away from the keyboard and back keeps
    /// its mapping.
    pub keys: BTreeMap<Button, String>,
    /// Which key pushes which stick, which way.
    ///
    /// Separate from `keys`: a button is a bit and an axis is a number.
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
/// Arrows for the directional pad, the home row for the face buttons, and a key of its own
/// for the shell button, so a person can reach the shell without reading anything.
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
/// Bound out of the box, because the default port is a keyboard and most titles are 3D. The
/// left stick takes the usual four keys and the right the block beside them, leaving the arrows
/// for the directional pad that moves the shell.
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
        // One keyboard port: zero would leave a fresh installation unable to press anything, and four
        // would put three empty pads in front of one keyboard.
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
    /// Growing adds empty ports and shrinking drops the last ones. Clamped to `1..=MAX_PORTS`,
    /// since zero ports leaves nothing to press.
    pub fn set_count(&mut self, count: usize) {
        let count = count.clamp(1, MAX_PORTS);
        while self.ports.len() < count {
            self.ports.push(Port::default());
        }
        self.ports.truncate(count);
    }

    /// Keys bound to more than one thing, on one port or across ports.
    ///
    /// Reported rather than resolved: letting the first binding win gives a binding that half
    /// works with nothing saying so.
    #[must_use]
    pub fn conflicts(&self) -> Vec<Conflict> {
        // One map across every port, so a key bound on two ports is reported: copying one port's
        // layout to another would otherwise make one person drive two pads (D341).
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
    /// Within a port it is usually a rebinding slip; across ports, two players move together.
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

    /// A fresh installation has one port somebody can use.
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

    /// A script-driven port survives the round trip through the configuration file.
    ///
    /// An internally tagged enum with a struct variant is the shape TOML is fussiest about, so the
    /// schema is pinned by a real round trip (D707).
    #[test]
    fn a_script_source_survives_the_config_round_trip() {
        let mut pads = Pads::default();
        pads.ports[0].source = Source::Script {
            path: "inputs/press-start.toml".to_owned(),
        };

        let text = toml::to_string(&pads).expect("a script-sourced port serialises");
        let back: Pads = toml::from_str(&text).expect("and reads back");
        assert_eq!(
            back.ports[0].source,
            Source::Script {
                path: "inputs/press-start.toml".to_owned()
            },
            "the source and its path came back unchanged, from:\n{text}"
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

    /// Changing the count keeps what was already set up.
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

    /// A key bound to two buttons is a reported conflict.
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

    /// A key bound on two ports is a reported conflict (D341).
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

    /// Opposite pushes sum to centre rather than one winning (D341).
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

    /// The shipped layout binds both sticks.
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
