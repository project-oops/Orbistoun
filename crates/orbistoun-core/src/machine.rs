//! Which machine orbistoun is presenting itself as.
//!
//! A domain type here because layers that cannot see each other both need it: the shell stores
//! it, and the kernel answers a guest from it.

use serde::{Deserialize, Serialize};

/// Which machine orbistoun presents itself as.
///
/// A guest asks separate questions (retail, devkit, faster revision) whose answers are not
/// independent, so the machine is the one setting and the answers are derived from it (D394).
/// It lives with what the hardware is set to rather than the installation's configuration,
/// because a title's behaviour belongs to the title and machine pairing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Machine {
    /// Which console generation.
    pub generation: Generation,
    /// Retail, development or test hardware.
    pub kind: Kind,
    /// Which hardware revision within the generation.
    pub revision: Revision,
    /// What the kernel calls its own release, as `kern.osrelease` answers it.
    ///
    /// A guest branches on it (`zftpd` reports `Firmware detection failed` and disables a
    /// feature), and the value is not in the FreeBSD checkout. Empty by default, and empty refuses
    /// the question rather than answering a guess (D397).
    #[serde(default)]
    pub kernel_release: String,
    /// What the system calls its own version, in the packed form a guest compares against.
    ///
    /// Open-toolchain payloads ask for it before bringing up their runtime linker, give up without
    /// it, and branch on it (against 7.00, 8.50, 9.30, 10.30). Packed major and minor in sixteen
    /// bits, so 12.40 is `0x1240`: what syscall 649 answers and `kern.version` names, distinct from
    /// [`Self::software_version`]. Zero means unset, and unset refuses the call (D403).
    #[serde(default)]
    pub firmware: u16,
    /// What `sceKernelGetSystemSwVersion` fills in - a different number from [`Self::firmware`].
    ///
    /// The reference machine runs system software 12.40, yet this call answers `13.090.001` /
    /// `0x1309_0001`, measured across three obSCEne module runs; the two are never derived from
    /// each other. `None` means unset, and unset refuses the call (D421).
    #[serde(default)]
    pub software_version: Option<SoftwareVersion>,
    /// What `kern.version` answers: the kernel's build banner, verbatim.
    ///
    /// One machine's string (`r226974/releases/12.40 Nov 27 2025 02:23:38`); only the release
    /// could be composed from [`Self::firmware`], so a profile states it whole. Empty is unset and
    /// refuses the knob (D675).
    #[serde(default)]
    pub kernel_version: String,
    /// What `kern.sdk_version` answers, as the integer the hardware wrote (`0x1240_0009`).
    ///
    /// Not derived from [`Self::firmware`], since the packing relating them is not documented.
    /// Zero is unset, and unset refuses (D675).
    #[serde(default)]
    pub kernel_sdk_version: u32,
    /// What `hw.model` answers: the processor's part name, space-padded as the hardware wrote it.
    ///
    /// Measured as `100-000000189` and 34 spaces, 47 bytes; the padding is part of the answer.
    /// Empty is unset, and unset refuses (D675).
    #[serde(default)]
    pub hardware_model: String,
}

/// The version `sceKernelGetSystemSwVersion` reports: the display string the guest reads and the
/// packed integer beside it.
///
/// A `char[0x1c]` string at offset 8 and a `uint32` at `0x24`, both measured; the rule relating
/// them is not documented, so a profile states both and one without the other fails to
/// deserialise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SoftwareVersion {
    /// The human string the call writes at offset 8, e.g. `13.090.001`. Truncated to the field's
    /// `0x1c` bytes if a profile gives a longer one.
    pub display: String,
    /// The packed integer the call writes at offset `0x24`, e.g. `0x1309_0001`.
    pub packed: u32,
}

/// Which console generation orbistoun presents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Generation {
    /// The later one, and the default: it is what this project is for.
    Orbis,
    /// The later one, and the default - it is what this project is for.
    #[default]
    Prospero,
}

/// Retail, development or test hardware.
///
/// An enum because exactly one is true, though a guest asks about each separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A retail unit. The default, because that is what the corpus is built for.
    #[default]
    Cex,
    /// A development kit.
    Dex,
    /// A test kit.
    Tex,
}

/// Which hardware revision within a generation.
///
/// The faster revision has more of everything and a title may branch on it. Independent of
/// [`Kind`]: a devkit is a devkit whichever revision it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Revision {
    /// The base machine, and the default.
    #[default]
    Base,
    /// The faster mid-generation revision.
    Pro,
}

/// Which machine a generation and a revision together name.
///
/// Derived rather than stored, so a `Machine` cannot claim two platforms at once. The
/// codenames are this project's vocabulary for the four machines and keep trademarks out of
/// orbistoun's own output (D663).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    /// The earlier generation's base machine.
    Orbis,
    /// The earlier generation's faster revision.
    Neo,
    /// The later generation's base machine, and what this project is for.
    Prospero,
    /// The later generation's faster revision.
    Trinity,
}

impl Platform {
    /// How to name it, in a report or a setting.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Orbis => "orbis",
            Self::Neo => "neo",
            Self::Prospero => "prospero",
            Self::Trinity => "trinity",
        }
    }
}

impl Machine {
    /// Whether a guest asking "is this retail" should be told yes.
    #[must_use]
    pub const fn is_retail(&self) -> bool {
        matches!(self.kind, Kind::Cex)
    }

    /// Whether a guest asking "is this a development kit" should be told yes.
    #[must_use]
    pub const fn is_development_kit(&self) -> bool {
        matches!(self.kind, Kind::Dex)
    }

    /// Whether a guest asking "is this a test kit" should be told yes.
    #[must_use]
    pub const fn is_test_kit(&self) -> bool {
        matches!(self.kind, Kind::Tex)
    }

    /// Whether this is the faster hardware revision.
    #[must_use]
    pub const fn is_faster_revision(&self) -> bool {
        matches!(self.revision, Revision::Pro)
    }

    /// Whether anything other than retail software is expected to run.
    ///
    /// True on development and test hardware: a guest asking this asks about the mode, not the
    /// box.
    #[must_use]
    pub const fn is_development_mode(&self) -> bool {
        !self.is_retail()
    }

    /// How to say what this is, for a run's own report.
    #[must_use]
    pub fn describe(&self) -> String {
        let generation = match self.generation {
            Generation::Orbis => "orbis",
            Generation::Prospero => "prospero",
        };
        let kind = match self.kind {
            Kind::Cex => "cex",
            Kind::Dex => "dex",
            Kind::Tex => "tex",
        };
        let revision = match self.revision {
            Revision::Base => "base",
            Revision::Pro => "pro",
        };
        format!("{generation}/{kind}/{revision}")
    }

    /// Which of the four machines this is.
    #[must_use]
    pub const fn platform(&self) -> Platform {
        match (self.generation, self.revision) {
            (Generation::Orbis, Revision::Base) => Platform::Orbis,
            (Generation::Orbis, Revision::Pro) => Platform::Neo,
            (Generation::Prospero, Revision::Base) => Platform::Prospero,
            (Generation::Prospero, Revision::Pro) => Platform::Trinity,
        }
    }
}

/// Which machine this process is presenting.
///
/// Published here because several crates answer from it without seeing each other: told once
/// by the layer that reads the settings file (D394).
static PRESENTED: std::sync::OnceLock<Machine> = std::sync::OnceLock::new();

/// Records which machine this process presents.
///
/// A second call is ignored, as for every process-wide table here.
pub fn present(machine: Machine) {
    let _ = PRESENTED.set(machine);
}

/// What this process presents itself as: a retail base Prospero machine when nothing configured
/// it.
#[must_use]
pub fn presented() -> &'static Machine {
    static DEFAULT: std::sync::OnceLock<Machine> = std::sync::OnceLock::new();
    PRESENTED
        .get()
        .unwrap_or_else(|| DEFAULT.get_or_init(Machine::default))
}

#[cfg(test)]
mod tests {

    /// Every generation and revision pair names its own platform (D663).
    #[test]
    fn each_generation_and_revision_names_its_own_platform() {
        let cases = [
            (Generation::Prospero, Revision::Base, Platform::Prospero),
            (Generation::Prospero, Revision::Pro, Platform::Trinity),
            (Generation::Orbis, Revision::Base, Platform::Orbis),
            (Generation::Orbis, Revision::Pro, Platform::Neo),
        ];
        let mut seen = Vec::new();
        for (generation, revision, expected) in cases {
            let machine = Machine {
                generation,
                revision,
                ..Machine::default()
            };
            assert_eq!(machine.platform(), expected);
            seen.push(machine.platform());
        }
        seen.sort_by_key(|p| p.label());
        seen.dedup();
        assert_eq!(seen.len(), 4, "four machines, four names, none shared");
    }

    /// A machine describes itself by codename, not by trademark.
    #[test]
    fn a_machine_describes_itself_by_codename() {
        let base = Machine::default();
        assert_eq!(base.describe(), "prospero/cex/base");
        assert!(
            !base.describe().contains("ps5") && !base.describe().contains("ps4"),
            "a trademark must not appear in what a run reports"
        );

        let pro = Machine {
            revision: Revision::Pro,
            ..Machine::default()
        };
        assert_eq!(pro.platform().label(), "trinity");
    }
    use super::{Generation, Kind, Machine, Platform, Revision};

    /// Exactly one kind is true (D393).
    #[test]
    fn exactly_one_kind_is_ever_true() {
        for kind in [Kind::Cex, Kind::Dex, Kind::Tex] {
            let machine = Machine {
                kind,
                ..Machine::default()
            };
            let said = [
                machine.is_retail(),
                machine.is_development_kit(),
                machine.is_test_kit(),
            ];
            assert_eq!(
                said.iter().filter(|yes| **yes).count(),
                1,
                "{kind:?} answered {said:?}, and a machine is one of the three"
            );
        }
    }

    /// Development mode is true on both kinds that are not retail, and only those.
    #[test]
    fn development_mode_is_anything_that_is_not_retail() {
        let retail = Machine::default();
        assert!(retail.is_retail() && !retail.is_development_mode());
        for kind in [Kind::Dex, Kind::Tex] {
            let machine = Machine {
                kind,
                ..Machine::default()
            };
            assert!(machine.is_development_mode(), "{kind:?}");
        }
    }

    /// The revision is independent of the kind - a devkit is one whichever revision it is.
    #[test]
    fn the_revision_is_independent_of_the_kind() {
        let pro_devkit = Machine {
            kind: Kind::Dex,
            revision: Revision::Pro,
            generation: Generation::Prospero,
            ..Machine::default()
        };
        assert!(pro_devkit.is_development_kit());
        assert!(pro_devkit.is_faster_revision());
        assert_eq!(pro_devkit.describe(), "prospero/dex/pro");
    }

    /// The firmware version is unset until somebody measures one (D397).
    #[test]
    fn nothing_pretends_to_know_the_firmware_version() {
        assert_eq!(
            Machine::default().firmware,
            0,
            "an unset version must refuse the call rather than choose a branch for the guest"
        );
    }

    /// The same rule for the release string, which a different call answers.
    #[test]
    fn nothing_pretends_to_know_the_kernel_release() {
        assert!(
            Machine::default().kernel_release.is_empty(),
            "an unset release must refuse the question rather than answer it wrongly"
        );
    }

    /// The same rule for the software version `sceKernelGetSystemSwVersion` answers.
    #[test]
    fn nothing_pretends_to_know_the_software_version() {
        assert!(
            Machine::default().software_version.is_none(),
            "an unset software version must refuse the call rather than invent one"
        );
    }

    /// The same rule for the kernel's build banner, SDK number and hardware model, each one
    /// machine's value (D675).
    #[test]
    fn nothing_pretends_to_know_the_kernel_banner_sdk_or_model() {
        let machine = Machine::default();
        assert!(
            machine.kernel_version.is_empty(),
            "kern.version is a build banner nobody here has read off this machine"
        );
        assert_eq!(
            machine.kernel_sdk_version, 0,
            "kern.sdk_version: zero is unset, and unset refuses"
        );
        assert!(
            machine.hardware_model.is_empty(),
            "hw.model names a part this machine has not been measured to have"
        );
    }

    /// The default is a retail base Prospero machine.
    #[test]
    fn the_default_is_what_every_measurement_was_taken_against() {
        assert_eq!(Machine::default().describe(), "prospero/cex/base");
    }
}
