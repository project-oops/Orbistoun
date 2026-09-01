//! Which machine orbistoun is presenting itself as.
//!
//! Here rather than with the rest of what the console is *set to*, because two layers that
//! cannot see each other both need it: the shell stores it, and the kernel answers a guest
//! from it. A domain type shared by every layer is exactly what this crate is for.

use serde::{Deserialize, Serialize};

/// Which machine orbistoun presents itself as.
///
/// # Why this is one setting and not five answers
///
/// A guest asks several separate questions - is this retail, is this a devkit, is this the
/// faster revision - and they are not independent: exactly one kind can be true, and a
/// machine that answered yes to two of them is not a machine. They were five hardcoded
/// constants in five functions, which is five places to be inconsistent and no place to say
/// what was intended.
///
/// So the *machine* is the setting and the answers are derived from it. A guest that takes a
/// devkit path takes it because somebody chose a devkit, and the run says which (D394).
///
/// It lives with the rest of what the console is **set to** rather than with the installation's
/// own configuration, because it travels: a title's behaviour on a retail PS5 is a fact about
/// that pairing, not about whose computer it ran on (D326).
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
    /// # Why this is empty by default rather than a plausible version
    ///
    /// A guest asks the kernel its version and **branches on the answer**: `zftpd` reports
    /// `Firmware detection failed` and disables a feature. Answering something plausible
    /// would send it down a path chosen by a number nobody measured, and the run would look
    /// like it worked.
    ///
    /// Nothing in this repository knows what a console's kernel calls itself - it is not in
    /// the FreeBSD checkout, because the console's kernel is not that kernel. So it is empty
    /// until somebody who has measured one fills it in, and empty means the question is
    /// refused rather than answered wrongly (D397).
    #[serde(default)]
    pub kernel_release: String,
    /// What the system calls its own version, in the packed form a guest compares against.
    ///
    /// # Why a guest cannot start without this
    ///
    /// Four open-toolchain payloads ask the kernel for it before they will bring up their own
    /// runtime linker, and give up when they get nothing - `Unable to initialize rtld`, then
    /// exit. Each then **branches on the value**, against 7.00, 8.50, 9.30 and 10.30, because
    /// what a payload does next depends on which system it is running on.
    ///
    /// So this is not a label. It selects a code path, and the wrong one is a guest doing
    /// something built for a different platform with nothing in a trace saying so.
    ///
    /// # Zero, and why that refuses rather than guesses
    ///
    /// The form is the one the guest reads: the major and minor in a sixteen-bit field, so
    /// 12.40 is `0x1240`. This is the *system software* version - what syscall 649 answers and
    /// what `kern.version` names. It is **not** what [`Self::software_version`] carries:
    /// `sceKernelGetSystemSwVersion` reports a different number on the same console (D420), which
    /// is why the two are separate settings.
    ///
    /// Zero means unset, and unset refuses the call. A plausible default would pick one of
    /// those branches for the guest and the run would look like it worked (D397, D403).
    #[serde(default)]
    pub firmware: u16,
    /// What `sceKernelGetSystemSwVersion` fills in - a *different* number from [`Self::firmware`].
    ///
    /// # Two versions, one console
    ///
    /// The reference machine runs system software 12.40 ([`Self::firmware`] = `0x1240`), yet this
    /// call answers `13.090.001` / `0x1309_0001` - measured across three obSCEne module runs, and
    /// deliberately not derived from the firmware, because on hardware the two simply differ
    /// (D420). Configurable for the same reason the firmware is: a different console presents a
    /// different value, and neither belongs compiled in (principle 5).
    ///
    /// `None` means unset, and unset refuses the call - the same honest default `firmware` and
    /// `kernel_release` keep, rather than inventing a version a guest would read back.
    #[serde(default)]
    pub software_version: Option<SoftwareVersion>,
}

/// The version `sceKernelGetSystemSwVersion` reports: the display string the guest reads and the
/// packed integer beside it.
///
/// **Both measured, neither derived from the other.** The structure holds a `char[0x1c]` string at
/// offset 8 and a `uint32` at offset 0x24, and the rule relating the two is not documented - one
/// sample is not a rule (principle 3). So a profile states both, and a profile that gives one
/// without the other is refused by deserialisation rather than half-answering.
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
    /// The earlier generation.
    Ps4,
    /// The later one, and the default - it is what this project is for.
    #[default]
    Ps5,
}

/// Retail, development or test hardware.
///
/// **Exactly one is true**, which is the whole reason these are an enum rather than three
/// booleans: a guest asks each separately, and answering yes to two describes nothing.
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
/// The faster revision has more of everything and a title may branch on it. Separate from
/// [`Kind`] because they are independent: a devkit is a devkit whichever revision it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Revision {
    /// The base machine, and the default.
    #[default]
    Base,
    /// The faster revision - a PS4 Pro, or a PS5 Pro.
    Pro,
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
    /// True on both development and test hardware, which is what separates it from
    /// [`Self::is_development_kit`] - a guest asking this is asking about the *mode*, not
    /// about the box.
    #[must_use]
    pub const fn is_development_mode(&self) -> bool {
        !self.is_retail()
    }

    /// How to say what this is, for a run's own report.
    #[must_use]
    pub fn describe(&self) -> String {
        let generation = match self.generation {
            Generation::Ps4 => "ps4",
            Generation::Ps5 => "ps5",
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
}

/// Which machine this process is presenting.
///
/// **Published here rather than in whichever crate answers a guest**, because several answer:
/// the kernel reports what kind of box it is, the C library reports what the kernel calls
/// itself, and neither of those crates can see the other. Told once, by the layer that reads
/// the settings file, and read wherever it is needed (D394, D397).
static PRESENTED: std::sync::OnceLock<Machine> = std::sync::OnceLock::new();

/// Records which machine this process presents.
///
/// A second call is ignored, as with every other process-wide table here: two machines in one
/// process is not something this supports.
pub fn present(machine: Machine) {
    let _ = PRESENTED.set(machine);
}

/// What this process presents itself as.
///
/// A retail base PS5 when nothing configured it, which is what every measurement so far was
/// taken against.
#[must_use]
pub fn presented() -> &'static Machine {
    static DEFAULT: std::sync::OnceLock<Machine> = std::sync::OnceLock::new();
    PRESENTED
        .get()
        .unwrap_or_else(|| DEFAULT.get_or_init(Machine::default))
}

#[cfg(test)]
mod tests {
    use super::{Generation, Kind, Machine, Revision};

    /// **Exactly one kind is true**, which is the property three booleans could not hold.
    ///
    /// The failure this protects against has happened: a platform that answered yes to
    /// retail *and* devkit, because each was a separate constant and one of them was a
    /// placeholder (D271, D393).
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
            generation: Generation::Ps5,
            ..Machine::default()
        };
        assert!(pro_devkit.is_development_kit());
        assert!(pro_devkit.is_faster_revision());
        assert_eq!(pro_devkit.describe(), "ps5/dex/pro");
    }

    /// **The kernel release is empty until somebody measures one.**
    ///
    /// A guest branches on it, so a plausible default would send it down a path chosen by a
    /// number nobody has ever seen - and the run would look like it worked (D397).
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

    /// And the same rule for the software version `sceKernelGetSystemSwVersion` answers.
    #[test]
    fn nothing_pretends_to_know_the_software_version() {
        assert!(
            Machine::default().software_version.is_none(),
            "an unset software version must refuse the call rather than invent one"
        );
    }

    /// The default is a retail base PS5, which is what every recorded measurement assumed.
    #[test]
    fn the_default_is_what_every_measurement_was_taken_against() {
        assert_eq!(Machine::default().describe(), "ps5/cex/base");
    }
}
