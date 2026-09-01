//! The console settings a person chooses, and the answers a guest reads.
//!
//! # Why this is the strongest reason to build a shell at all
//!
//! `orbistoun-systemservice` answers a title's questions about the machine - what language
//! it is set to, which button confirms - with a documented placeholder, because *"nothing
//! here knows what any of these parameters mean; they are console settings"*.
//!
//! They are console settings, and a console shell is where a person sets them. That makes
//! this the file where a whole class of placeholder stops being a placeholder: not by
//! guessing better, but by **asking the only party who actually knows** what they want the
//! machine to be.
//!
//! # Two unknowns, kept apart
//!
//! Answering a title still needs two things this repository is not entitled to invent: the
//! *identifier* it asks by, and the *encoding* of the answer. Both are measurements.
//!
//! So [`Settings`] holds meanings a person picked and is entirely ours, while
//! [`Parameters`] holds numbers and is **empty until something measures them**. A setting
//! with no measured encoding changes what the shell displays and nothing about what the
//! guest is told - which is the honest position, and it improves the moment a code is
//! measured rather than requiring this file to be rewritten (principle 5).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Which button confirms.
///
/// **Named by position, not by glyph.** The two conventions differ by region and the
/// vendor's own names for these buttons are its own; a direction is what the hardware
/// actually presents and what every other layer here can act on without a lookup
/// (principle 2).
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
/// Every field is something the owner of the emulator genuinely decides, which is what
/// separates this from a table of guesses. Nothing here is read from a title, a firmware
/// image, or anything else this repository may not hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Preferred language, as a BCP 47 tag - `en-GB`, `ja-JP`.
    ///
    /// A published standard rather than the platform's own numbering, so the value is
    /// meaningful on its own. Turning it into whatever a guest expects is a measurement,
    /// and lives in [`Parameters`].
    pub language: String,
    /// Which button confirms.
    pub confirm: ButtonAssignment,
    /// Who this machine belongs to.
    ///
    /// **A list, because a console has users rather than a user.** A title asks for a name
    /// by identifier, enumerates who is signed in, and keys its save data on which one -
    /// none of which a single name can answer. Modelling it as one field would have worked
    /// until the first title asked a question about the second person (D346).
    pub users: Vec<User>,
    /// The highest identifier ever handed out, so one is never reused.
    ///
    /// Stored rather than derived: the highest *live* identifier drops when its holder is
    /// deleted, and the next user would take the number - and the saves - of somebody who
    /// is gone.
    #[serde(default)]
    pub issued: u32,
    /// Which user is signed in, by [`User::id`].
    ///
    /// One at a time. A console supports several signed in at once and nothing here models
    /// that, because nothing has asked yet - and a list of active users is a different thing
    /// from a list of accounts, which is what is above.
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
    /// **Ours to choose, and one-based**, for the reason `orbistoun-core::Handle` is: a guest
    /// that zero-initialises an identifier field and then tests it must not find a valid user
    /// there. Stable across restarts because it is stored, which matters more than it sounds -
    /// a title keys save data on this, so a number that moved would lose somebody's saves.
    pub id: u32,
    /// What to call them.
    pub name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            machine: orbistoun_core::machine::Machine::default(),
            // The host's locale is not consulted, deliberately: a setting that changes with
            // the machine it runs on makes two runs of the same title incomparable, and
            // comparability is most of what this emulator's reports are for.
            language: "en-GB".to_owned(),
            confirm: ButtonAssignment::default(),
            // One user, because a fresh machine that nobody can be signed in as is a machine
            // no title can start on - and four empty accounts in front of one person is
            // worse than one they can rename.
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
    /// **`None` is reachable and has to be handled**: deleting the signed-in user in the
    /// settings window leaves an identifier pointing at nobody, and answering a title with
    /// a name belonging to a deleted account would be worse than answering nothing.
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
    /// **A stored high-water mark rather than the highest live identifier**, and the
    /// difference is the whole point. Deriving it from the list reuses a number the moment
    /// its holder is deleted - which the test for this caught, having been written first.
    /// A title keys save data on this, so a reused number silently hands somebody else's
    /// saves to a new person.
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
    /// When the file exists and cannot be parsed. **Not silent**: a malformed file that
    /// fell back to defaults would look exactly like a machine nobody had configured, and
    /// the person who edited it would be debugging the wrong thing.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("reading {}: {e}", path.display())),
        };
        toml::from_str(&text).map_err(|e| format!("parsing {}: {e}", path.display()))
    }

    /// The file as text, for writing it back.
    ///
    /// # Errors
    ///
    /// When the settings cannot be serialised.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| e.to_string())
    }
}

/// How one parameter identifier is answered.
///
/// **Every number in here is measured.** The variants differ in what they are measurements
/// *of*: a constant nobody has tied to a setting yet, or an encoding that lets a setting
/// drive the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "kebab-case")]
pub enum Answer {
    /// A measured constant, independent of any setting.
    ///
    /// Where most entries will start: something observed to be asked for and observed to
    /// be answered a particular way, before anybody knows which menu it corresponds to.
    Fixed {
        /// The value the guest reads.
        value: i32,
    },
    /// The confirm button, with the encoding for each position measured.
    ///
    /// The shape every setting-backed parameter takes: the *meaning* comes from
    /// [`Settings`] and both *numbers* come from measurement, so neither half is invented.
    Confirm {
        /// What the guest reads when the lower face button confirms.
        south: i32,
        /// What the guest reads when the right face button confirms.
        east: i32,
    },
}

/// Which identifiers can be answered, and how.
///
/// **Empty by default.** Each entry is a claim about the vendor's interface, so it arrives
/// by measurement and is loaded rather than compiled (principle 5).
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
    /// `None` means **nothing measured says what this is**, which the caller must report
    /// rather than paper over. The shim's placeholder is still the right value to hand
    /// back; what it must not do is hand it back silently, because a placeholder that is
    /// never counted is indistinguishable from an answer (principle 3).
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

    /// **The shipped state answers nothing, and that is the claim being protected.**
    ///
    /// If this test is ever changed because entries were added to the default table, the
    /// first question is where those numbers were measured - not whether the test is
    /// inconvenient.
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

    /// **A person's choice reaches the guest once, and only once, the encoding is known.**
    ///
    /// This is the whole argument for building a shell, reduced to an assertion: the
    /// meaning is ours, the numbers are measured, and the answer changes when the person
    /// changes their mind.
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
            // A machine that is not the default, so the round trip is asserting something.
            machine: orbistoun_core::machine::Machine {
                generation: orbistoun_core::machine::Generation::Ps4,
                kind: orbistoun_core::machine::Kind::Dex,
                revision: orbistoun_core::machine::Revision::Pro,
                kernel_release: "measured-somewhere".to_owned(),
                firmware: 0x1234,
                software_version: Some(orbistoun_core::machine::SoftwareVersion {
                    display: "1.02.003".to_owned(),
                    packed: 0x0102_0003,
                }),
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

    /// **A signed-in identifier that names nobody answers nothing.**
    ///
    /// Reachable by deleting the signed-in user in the settings window, and the alternative
    /// is worse than an empty answer: handing a title the name of a deleted account.
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

    /// **An identifier is never reused, because save data is keyed on it.**
    ///
    /// Handing a deleted user's number to a new person would silently give them somebody
    /// else's saves - the same reasoning that stops handles being recycled.
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

    /// **The default language does not follow the host.**
    ///
    /// A setting that varies with the machine makes two runs of the same title
    /// incomparable, and comparing runs is most of what this emulator's reports do.
    #[test]
    fn the_default_language_is_fixed_rather_than_taken_from_the_host() {
        assert_eq!(Settings::default().language, "en-GB");
    }
}
