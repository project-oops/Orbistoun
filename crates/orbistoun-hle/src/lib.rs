//! The HLE boundary: module descriptions, the import registry, and stub policy.
//!
//! Every `orbistoun-<subsystem>` crate declares what it implements with [`guest_module!`], and
//! this crate turns those declarations into a NID-keyed table of everything orbistoun can answer.
//! The loader resolves each guest import against it and writes the result into the relocation
//! slots, so linking is the interception and a title's needs are known before it runs (D005).
//! What an unimplemented function returns is a runtime-editable TOML file ([`StubPolicy`]),
//! keyed by symbol name, so bisecting a function's semantics is a file edit and a relaunch.
//!
//! ```
//! use orbistoun_hle::{ModuleDesc, guest_module};
//!
//! guest_module! {
//!     "libExample" {
//!         "exampleInit" => 0,
//!         "exampleOpen" => 4,
//!     }
//! }
//!
//! assert_eq!(MODULE.name, "libExample");
//! assert_eq!(MODULE.imports.len(), 2);
//! assert_eq!(MODULE.imports[1].arity, 4);
//! ```

pub mod clocks;
pub mod constants;
pub mod differential;
pub mod hardware;
pub mod knowledge;
pub mod learned;
pub mod origin;

use std::collections::HashMap;

use crate::knowledge::Oracle;
use orbistoun_core::GuestError;
use orbistoun_nid::{Nid, NidHasher};
use serde::{Deserialize, Serialize};

/// Why a file this crate owns could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum HleError {
    /// The file exists and could not be read.
    #[error("reading {}: {source}", path.display())]
    Read {
        /// The file.
        path: std::path::PathBuf,
        /// What the filesystem said.
        source: std::io::Error,
    },
    /// The file was read and is not the format it should be.
    #[error("parsing {}: {source}", path.display())]
    Parse {
        /// The file.
        path: std::path::PathBuf,
        /// What the parser said, boxed to keep the error small.
        source: Box<toml::de::Error>,
    },
    /// The contents could not be rendered as TOML.
    #[error("serialising TOML: {0}")]
    Toml(#[from] toml::ser::Error),
}

/// One function a subsystem crate declares.
///
/// No NID: it is derived from `name` at registration with the runtime hash suffix, so a
/// declaration cannot carry a NID that disagrees with its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportDesc {
    /// The symbol name, exactly as the firmware exports it.
    pub name: &'static str,
    /// How many integer arguments the function takes.
    ///
    /// Decides how many argument registers a trace records; a wrong arity degrades the trace, not
    /// the call.
    pub arity: u8,
}

/// One target system library, as far as orbistoun models it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleDesc {
    /// Library name as it appears in a guest import table, e.g. `libSceAudioOut`.
    pub name: &'static str,
    /// Everything this crate declares for that library.
    pub imports: &'static [ImportDesc],
}

/// Declares a system library and the functions orbistoun knows about.
///
/// Expands to a `pub const MODULE: ModuleDesc`, one per subsystem crate. `modules()` in
/// `orbistoun-service` is the single list that hands them to the [`Registry`].
#[macro_export]
macro_rules! guest_module {
    ($lib:literal { $($name:literal => $arity:literal),* $(,)? }) => {
        /// This crate's module description, consumed by the HLE registry.
        ///
        // `unreachable_pub` is allowed because the macro is also used inside private modules (tests,
        // and subsystems that group libraries into submodules).
        #[allow(unreachable_pub)]
        pub const MODULE: $crate::ModuleDesc = $crate::ModuleDesc {
            name: $lib,
            imports: &[
                $($crate::ImportDesc { name: $name, arity: $arity }),*
            ],
        };
    };
}

/// What to return to the guest from a function with no implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StubReturn {
    /// Report success. Usually the right first guess, and usually wrong later.
    Ok,
    /// Report the generic unimplemented marker, which is loud in a trace.
    Unimplemented,
    /// Report a specific raw code, once one has been established.
    Raw(u32),
}

impl StubReturn {
    /// The value the guest observes.
    pub const fn as_raw(self) -> u32 {
        match self {
            Self::Ok => 0,
            Self::Unimplemented => GuestError::Unimplemented.as_raw(),
            Self::Raw(v) => v,
        }
    }
}

/// Per-function stub behaviour, loaded from TOML.
///
/// Keyed by symbol name so the file is editable by a human without a NID table
/// to hand. Anything not named falls back to [`StubPolicy::default_return`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StubPolicy {
    /// Applied to every function with no explicit entry.
    pub default_return: StubReturn,
    /// Overrides, by symbol name.
    #[serde(default)]
    pub overrides: HashMap<String, StubReturn>,
    /// What a stub writes, by symbol name.
    ///
    /// An override says what a function answers; this says what it does, often the part a guest
    /// needs. The shape is what a sweep produces, so a measured `OutParameter` becomes an entry with
    /// no judgement in between; the loop may write data, never code (D296).
    #[serde(default)]
    pub regions: HashMap<String, StubRegion>,
    /// How each entry above was established, by symbol name.
    ///
    /// Beside the answers rather than inside them: the answer is what the guest observes and this is
    /// what a report observes. A missing name reads as [`Oracle::Assumed`], so an unlabelled answer
    /// can only make a run look less honest (D557).
    #[serde(default)]
    pub known: HashMap<String, Oracle>,
}

/// How a region reaches the guest.
///
/// Writing a base into an argument and returning it are the same behaviour delivered
/// differently, so they are one type with a field (D300).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Delivery {
    /// Written through the address held in this argument register.
    Argument(u8),
    /// Handed back as the function's answer.
    Return,
}

/// What an unimplemented function stores before it answers.
///
/// One write, of one base, into one argument: the shape a sweep can measure. Arbitrary side
/// effects would make the policy file a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StubRegion {
    /// How the base reaches the guest.
    pub via: Delivery,
    /// How much space to reserve behind it.
    ///
    /// Assumed: a sweep sees where the guest faulted, not how much it meant to use. A number in a
    /// file, so it can be changed and re-run without a rebuild (D291).
    pub bytes: u64,
}

impl Default for StubPolicy {
    fn default() -> Self {
        // Unimplemented by default, not Ok: a silent success turns a wrong stub into a hang long
        // afterwards. Individual functions are relaxed deliberately.
        Self {
            default_return: StubReturn::Unimplemented,
            overrides: HashMap::new(),
            regions: HashMap::new(),
            known: HashMap::new(),
        }
    }
}

impl StubPolicy {
    /// Folds policy the loop worked out into policy a person wrote.
    ///
    /// A person's entry always wins, so the loop running unattended can never override a deliberate
    /// choice (D296). `default_return` is never taken from the learned side, since it governs every
    /// function the loop did not measure.
    pub fn absorb(&mut self, learned: Self) {
        for (name, answer) in learned.overrides {
            self.overrides.entry(name).or_insert(answer);
        }
        for (name, region) in learned.regions {
            self.regions.entry(name).or_insert(region);
        }
        // Provenance follows the same rule as the answer it describes: `or_insert` leaves a person's
        // unlabelled entry absent, which reads as `Assumed`, so a measured fact never relabels a
        // hand-written answer as evidence.
        for (name, known) in learned.known {
            self.known.entry(name).or_insert(known);
        }
    }

    /// How `name`'s answer was established; [`Oracle::Assumed`] when nothing says (see
    /// [`StubPolicy::known`]).
    #[must_use]
    pub fn provenance(&self, name: &str) -> Oracle {
        self.known.get(name).copied().unwrap_or(Oracle::Assumed)
    }

    /// Every symbol this policy says anything specific about, once each.
    ///
    /// Answers and regions together, so a policy that only writes guest memory still counts
    /// (D557).
    pub fn named(&self) -> impl Iterator<Item = &str> {
        let mut names: Vec<&str> = self
            .overrides
            .keys()
            .chain(self.regions.keys())
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names.dedup();
        names.into_iter()
    }

    /// How many symbols this policy answers or writes for, once each.
    #[must_use]
    pub fn specific(&self) -> usize {
        self.named().count()
    }

    /// Of those, how many hold the run up rather than describing it.
    ///
    /// An entry whose provenance [`Oracle::is_evidence`] is the emulator being right; the rest are
    /// props, and a run resting on one is an experiment rather than a measurement.
    #[must_use]
    pub fn propping(&self) -> usize {
        self.named()
            .filter(|n| !self.provenance(n).is_evidence())
            .count()
    }

    /// The behaviour configured for `name`.
    pub fn for_symbol(&self, name: &str) -> StubReturn {
        self.overrides
            .get(name)
            .copied()
            .unwrap_or(self.default_return)
    }
}

/// A resolved import: what the guest asked for, and what we know about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Library the symbol belongs to.
    pub library: &'static str,
    /// Symbol name.
    pub name: &'static str,
    /// Argument count, for trace fidelity.
    pub arity: u8,
    /// What a call returns while it has no implementation.
    pub stub: StubReturn,
}

/// Everything orbistoun can answer, keyed by NID.
///
/// Built once at startup from every subsystem crate's `MODULE`, then queried by the loader for
/// each import a guest module names.
#[derive(Debug)]
pub struct Registry {
    hasher: NidHasher,
    policy: StubPolicy,
    by_nid: HashMap<Nid, Resolved>,
}

impl Registry {
    /// Creates an empty registry that will hash names with `hasher`.
    pub fn new(hasher: NidHasher, policy: StubPolicy) -> Self {
        Self {
            hasher,
            policy,
            by_nid: HashMap::new(),
        }
    }

    /// The hasher this registry resolves with.
    ///
    /// Exposed so readers hash plain-name imports with the same suffix; a different one yields NIDs
    /// that silently match nothing (D305).
    #[must_use]
    pub const fn hasher(&self) -> &NidHasher {
        &self.hasher
    }

    /// Registers every import in `module`.
    ///
    /// Later registrations win on collision, so a real implementation displaces a stub without
    /// unregistering it.
    pub fn register(&mut self, module: ModuleDesc) {
        for import in module.imports {
            let nid = if let Some(hex) = import.name.strip_prefix("0x") {
                if let Ok(raw) = u64::from_str_radix(hex, 16) {
                    Nid::from_raw(raw)
                } else {
                    self.hasher.hash(import.name)
                }
            } else {
                self.hasher.hash(import.name)
            };
            self.by_nid.insert(
                nid,
                Resolved {
                    library: module.name,
                    name: import.name,
                    arity: import.arity,
                    stub: self.policy.for_symbol(import.name),
                },
            );
        }
    }

    /// Looks up what orbistoun knows about `nid`.
    ///
    /// `None` means the guest imported something declared nowhere, which an import dump reports.
    pub fn resolve(&self, nid: Nid) -> Option<&Resolved> {
        self.by_nid.get(&nid)
    }

    /// How many functions are registered.
    pub fn len(&self) -> usize {
        self.by_nid.len()
    }

    /// Whether anything is registered.
    pub fn is_empty(&self) -> bool {
        self.by_nid.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Delivery, Oracle, Registry, StubPolicy, StubRegion, StubReturn};
    use orbistoun_nid::NidHasher;
    use std::collections::HashMap;

    /// A person's entry always wins over one the loop worked out (D296).
    #[test]
    fn a_deliberate_entry_is_never_overridden_by_a_learned_one() {
        let mut mine = StubPolicy {
            default_return: StubReturn::Unimplemented,
            overrides: [("sceFoo".to_owned(), StubReturn::Raw(0x1234))]
                .into_iter()
                .collect(),
            regions: HashMap::new(),
            known: HashMap::new(),
        };
        let learned = StubPolicy {
            // Never taken: it governs every function the loop did not measure.
            default_return: StubReturn::Ok,
            overrides: [
                ("sceFoo".to_owned(), StubReturn::Ok),
                ("sceBar".to_owned(), StubReturn::Ok),
            ]
            .into_iter()
            .collect(),
            regions: [(
                "sceBar".to_owned(),
                StubRegion {
                    via: Delivery::Argument(0),
                    bytes: 0x1000,
                },
            )]
            .into_iter()
            .collect(),
            // The loop's entries say where they came from: a sweep watches the guest proceed, which is
            // `GuestObserved` and never stronger.
            known: [
                ("sceFoo".to_owned(), Oracle::GuestObserved),
                ("sceBar".to_owned(), Oracle::GuestObserved),
            ]
            .into_iter()
            .collect(),
        };
        mine.absorb(learned);

        assert_eq!(
            mine.for_symbol("sceFoo"),
            StubReturn::Raw(0x1234),
            "a deliberate entry must survive"
        );
        assert_eq!(
            mine.for_symbol("sceBar"),
            StubReturn::Ok,
            "and one the loop added, where nothing deliberate existed, must land"
        );
        assert!(mine.regions.contains_key("sceBar"), "including its region");
        assert_eq!(
            mine.default_return,
            StubReturn::Unimplemented,
            "the fallback for everything unmeasured is never the loop's to change"
        );
    }

    guest_module! {
        "libTest" {
            "testInit" => 0,
            "testOpen" => 3,
        }
    }

    fn registry(policy: StubPolicy) -> Registry {
        let mut r = Registry::new(NidHasher::new(*b"test-suffix"), policy);
        r.register(MODULE);
        r
    }

    /// Declared symbols resolve by NID.
    #[test]
    fn declared_symbols_resolve_by_nid() {
        let hasher = NidHasher::new(*b"test-suffix");
        let r = registry(StubPolicy::default());

        let found = r.resolve(hasher.hash("testOpen")).expect("declared");
        assert_eq!(found.library, "libTest");
        assert_eq!(found.arity, 3);
        assert_eq!(r.len(), 2);
    }

    /// Undeclared symbols resolve to nothing.
    #[test]
    fn undeclared_symbols_resolve_to_nothing() {
        let hasher = NidHasher::new(*b"test-suffix");
        let r = registry(StubPolicy::default());
        assert!(r.resolve(hasher.hash("testNotDeclared")).is_none());
    }

    /// The default policy answers the unimplemented marker, not silent success.
    #[test]
    fn default_policy_is_loud_not_silent_success() {
        // A stub that reports success is indistinguishable from working code until much later.
        let r = registry(StubPolicy::default());
        let hasher = NidHasher::new(*b"test-suffix");
        let found = r.resolve(hasher.hash("testInit")).expect("declared");
        assert_eq!(found.stub, StubReturn::Unimplemented);
    }

    /// A policy override applies to its own symbol only.
    #[test]
    fn policy_overrides_apply_per_symbol() {
        let mut policy = StubPolicy::default();
        policy
            .overrides
            .insert("testInit".to_owned(), StubReturn::Ok);
        let r = registry(policy);
        let hasher = NidHasher::new(*b"test-suffix");

        assert_eq!(
            r.resolve(hasher.hash("testInit")).expect("declared").stub,
            StubReturn::Ok
        );
        // The override must not leak to its neighbours, or bisection breaks.
        assert_eq!(
            r.resolve(hasher.hash("testOpen")).expect("declared").stub,
            StubReturn::Unimplemented
        );
    }

    /// A raw-hex NID symbol resolves by that NID.
    #[test]
    fn raw_hex_nid_symbols_resolve_by_raw_nid() {
        guest_module! {
            "libRaw" {
                "0x7d86501b8094ef57" => 1,
            }
        }
        let mut r = Registry::new(NidHasher::new(*b"test-suffix"), StubPolicy::default());
        r.register(MODULE);
        let found = r
            .resolve(orbistoun_nid::Nid::from_raw(0x7d86_501b_8094_ef57))
            .expect("raw nid declared");
        assert_eq!(found.library, "libRaw");
        assert_eq!(found.name, "0x7d86501b8094ef57");
        assert_eq!(found.arity, 1);
    }
}
