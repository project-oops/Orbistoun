//! The system settings a person chooses, and the answers a guest reads.
//!
//! A title's questions about the machine (language, which button confirms) are answered
//! from what a person set here. [`Settings`] holds those meanings and is entirely ours;
//! [`Parameters`] holds the identifier a guest asks by and the encoding of the answer,
//! which are measurements and empty by default. A setting with no measured encoding changes
//! what the shell displays and nothing the guest is told.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ShellError;

/// Which button confirms.
///
/// Named by position, not by the vendor's glyph names; the convention differs by region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ButtonAssignment {
    /// The lower face button confirms.
    #[default]
    South,
    /// The right face button confirms.
    East,
}

/// What the machine is set to, as a person set it.
///
/// Every field is chosen by the owner of the emulator; nothing is read from a title or a
/// firmware image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Preferred language, as a BCP 47 tag - `en-GB`, `ja-JP`.
    ///
    /// A published standard rather than the platform's numbering; the encoding a guest
    /// expects is a measurement in [`Parameters`].
    pub language: String,
    /// Which button confirms.
    pub confirm: ButtonAssignment,
    /// Who this machine belongs to.
    ///
    /// A list, because a title asks for a name by identifier, enumerates who is signed in
    /// and keys its save data on which one (D346).
    pub users: Vec<User>,
    /// The highest identifier ever handed out, so one is never reused.
    ///
    /// Stored rather than derived: the highest live identifier drops when its holder is
    /// deleted, and the next user would take that number and its saves.
    #[serde(default)]
    pub issued: u32,
    /// Which user is signed in, by [`User::id`].
    ///
    /// One at a time; several simultaneous sign-ins are not modelled.
    pub signed_in: u32,
    /// Which machine this is presenting itself as.
    #[serde(default)]
    pub machine: orbistoun_core::machine::Machine,
}

/// One person this machine belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// The identifier a title sees.
    ///
    /// Ours to choose, and one-based like `orbistoun-core::Handle`, so a zero-initialised
    /// field never names a valid user. Stored, so it is stable across restarts: a title keys
    /// save data on it.
    pub id: u32,
    /// What to call them.
    pub name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            machine: orbistoun_core::machine::Machine::default(),
            // The host's locale is not consulted, so runs on different hosts compare.
            language: "en-GB".to_owned(),
            confirm: ButtonAssignment::default(),
            // One user, because a title cannot start on a machine nobody is signed in to.
            users: vec![User {
                id: 1,
                name: "player".to_owned(),
            }],
            signed_in: 1,
            issued: 1,
        }
    }
}

impl Settings {
    /// The user who is signed in, if the identifier still names one.
    ///
    /// `None` is reachable: deleting the signed-in user leaves an identifier naming nobody.
    #[must_use]
    pub fn current(&self) -> Option<&User> {
        self.user(self.signed_in)
    }

    /// The user with this identifier.
    #[must_use]
    pub fn user(&self, id: u32) -> Option<&User> {
        self.users.iter().find(|user| user.id == id)
    }

    /// Takes the next identifier, and never hands the same one out twice.
    ///
    /// A stored high-water mark rather than the highest live identifier, so a deleted
    /// user's number, and the save data keyed on it, never passes to a new user.
    pub fn take_id(&mut self) -> u32 {
        self.issued = self.issued.max(
            self.users
                .iter()
                .map(|user| user.id)
                .max()
                .unwrap_or_default(),
        );
        self.issued += 1;
        self.issued
    }

    /// Reads a settings file, or the defaults when there is none.
    ///
    /// # Errors
    ///
    /// When the file exists and cannot be read or parsed. A malformed file is an error
    /// rather than a silent fall-back to defaults.
    pub fn load(path: &std::path::Path) -> Result<Self, ShellError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(ShellError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        toml::from_str(&text).map_err(|source| ShellError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })
    }

    /// The file as text, for writing it back.
    ///
    /// # Errors
    ///
    /// When the settings cannot be serialised.
    pub fn to_toml(&self) -> Result<String, ShellError> {
        Ok(toml::to_string_pretty(self)?)
    }
}

/// How one parameter identifier is answered.
///
/// Every number here is measured: either a constant not tied to a setting, or an encoding
/// that lets a setting drive the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "kebab-case")]
pub enum Answer {
    /// A measured constant, independent of any setting.
    ///
    /// An observed answer not yet tied to any setting.
    Fixed {
        /// The value the guest reads.
        value: i32,
    },
    /// The confirm button, with the encoding for each position measured.
    ///
    /// The meaning comes from [`Settings`] and both numbers from measurement.
    Confirm {
        /// What the guest reads when the lower face button confirms.
        south: i32,
        /// What the guest reads when the right face button confirms.
        east: i32,
    },
}

/// Which identifiers can be answered, and how.
///
/// Empty by default. Each entry is a claim about the vendor's interface, so it comes from
/// measurement and is loaded rather than compiled.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parameters {
    /// One answer per identifier. Absent means unanswerable.
    #[serde(default, rename = "parameter")]
    answers: BTreeMap<u32, Answer>,
}

impl Parameters {
    /// A table that can answer nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Records a measured answer.
    pub fn set(&mut self, id: u32, answer: Answer) {
        self.answers.insert(id, answer);
    }

    /// What to tell a guest that asked for this identifier.
    ///
    /// `None` means nothing measured answers it. The caller still returns its placeholder
    /// but must report it, so a placeholder is never mistaken for an answer.
    #[must_use]
    pub fn answer(&self, id: u32, settings: &Settings) -> Option<i32> {
        match self.answers.get(&id)? {
            Answer::Fixed { value } => Some(*value),
            Answer::Confirm { south, east } => Some(match settings.confirm {
                ButtonAssignment::South => *south,
                ButtonAssignment::East => *east,
            }),
        }
    }

    /// Whether anything at all can be answered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.answers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Answer, ButtonAssignment, Parameters, Settings};

    /// The default table answers nothing: every entry must come from measurement.
    #[test]
    fn nothing_is_answerable_until_something_has_been_measured() {
        let table = Parameters::empty();
        assert!(table.is_empty());
        assert_eq!(
            table.answer(0, &Settings::default()),
            None,
            "an unmeasured identifier has no answer, and saying so is the point"
        );
    }

    /// A person's choice reaches the guest only once the encoding is measured, and follows
    /// the setting when it changes.
    #[test]
    fn a_setting_drives_the_answer_when_its_encoding_has_been_measured() {
        let mut table = Parameters::empty();
        table.set(1000, Answer::Confirm { south: 0, east: 1 });

        let confirming = |confirm| Settings {
            confirm,
            ..Settings::default()
        };

        let settings = confirming(ButtonAssignment::South);
        assert_eq!(table.answer(1000, &settings), Some(0));

        let settings = confirming(ButtonAssignment::East);
        assert_eq!(
            table.answer(1000, &settings),
            Some(1),
            "changing the setting changes what the title is told"
        );
    }

    /// A measured constant is answered without reference to any setting.
    #[test]
    fn a_fixed_answer_does_not_depend_on_what_anybody_chose() {
        let mut table = Parameters::empty();
        table.set(42, Answer::Fixed { value: 7 });

        let settings = Settings {
            confirm: ButtonAssignment::East,
            ..Settings::default()
        };
        assert_eq!(table.answer(42, &settings), Some(7));
    }

    /// Settings survive being written and read back, including a non-default choice.
    #[test]
    fn settings_survive_a_round_trip() {
        let settings = Settings {
            confirm: ButtonAssignment::East,
            language: "ja-JP".to_owned(),
            // A non-default machine, so the round trip asserts something.
            machine: orbistoun_core::machine::Machine {
                generation: orbistoun_core::machine::Generation::Orbis,
                kind: orbistoun_core::machine::Kind::Dex,
                revision: orbistoun_core::machine::Revision::Pro,
                kernel_release: "measured-somewhere".to_owned(),
                firmware: 0x1234,
                software_version: Some(orbistoun_core::machine::SoftwareVersion {
                    display: "1.02.003".to_owned(),
                    packed: 0x0102_0003,
                }),
                // Non-default too, so the round trip covers the three sysctl knobs' keys (D675).
                kernel_version: "a banner somebody read".to_owned(),
                kernel_sdk_version: 0x0102_0003,
                hardware_model: "a padded model   ".to_owned(),
            },
            users: vec![
                super::User {
                    id: 1,
                    name: "someone".to_owned(),
                },
                super::User {
                    id: 7,
                    name: "somebody else".to_owned(),
                },
            ],
            signed_in: 7,
            issued: 7,
        };

        let text = settings.to_toml().expect("serialises");
        let back: Settings = toml::from_str(&text).expect("parses");

        assert_eq!(back, settings);
    }

    /// A signed-in identifier that names nobody answers nothing.
    #[test]
    fn a_deleted_signed_in_user_is_not_answered_with_somebody_else() {
        let settings = Settings {
            signed_in: 99,
            ..Settings::default()
        };

        assert!(settings.current().is_none());
        assert!(
            settings.user(1).is_some(),
            "the account itself is still there"
        );
    }

    /// An identifier is never reused, because save data is keyed on it.
    #[test]
    fn a_new_user_never_takes_a_deleted_ones_identifier() {
        let mut settings = Settings::default();
        let second = settings.take_id();
        settings.users.push(super::User {
            id: second,
            name: "two".to_owned(),
        });

        settings.users.retain(|user| user.id != second);
        assert!(
            settings.take_id() > second,
            "the number the deleted user held is not offered again"
        );
    }

    /// The default language does not follow the host, so runs stay comparable.
    #[test]
    fn the_default_language_is_fixed_rather_than_taken_from_the_host() {
        assert_eq!(Settings::default().language, "en-GB");
    }
}
