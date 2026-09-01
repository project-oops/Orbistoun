//! The HLE boundary: module descriptions, the import registry, and stub policy.
//!
//! Every `orbistoun-<subsystem>` crate declares what it implements with the
//! [`guest_module!`] macro and nothing else. This crate turns those declarations
//! into the thing the loader needs: a NID-keyed table of everything orbistoun can
//! answer, plus a per-function policy for what to return when it cannot.
//!
//! # Interception is linking, not hooking
//!
//! There is no instrumentation step here. A guest module imports by NID; the
//! loader resolves each NID against this registry and writes the result into the
//! guest's relocation slots. Being the linker *is* the interception, which is why
//! the complete list of what a title needs is available statically, before a
//! single guest instruction executes.
//!
//! # Stub policy is data
//!
//! The return value of an unimplemented function changes guest behaviour
//! enormously - zero means "carried on", a negative code means "bailed out" - and
//! which one is right is usually unknown. So it is a runtime-editable TOML file
//! ([`StubPolicy`]), keyed by human-readable symbol name, not a recompile. That
//! makes bisecting a function's semantics a file edit and a relaunch.
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

pub mod constants;
pub mod knowledge;
pub mod learned;

use std::collections::HashMap;

use orbistoun_core::GuestError;
use orbistoun_nid::{Nid, NidHasher};
use serde::{Deserialize, Serialize};

/// One function a subsystem crate declares.
///
/// The NID is deliberately absent: it is derived from `name` at registration
/// time, because the hash suffix is runtime data (see `orbistoun-nid`). That also
/// means a declaration can never carry a NID that disagrees with its own name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportDesc {
    /// The symbol name, exactly as the firmware exports it.
    pub name: &'static str,
    /// How many integer arguments the function takes.
    ///
    /// Used to decide how many argument registers are worth recording in a
    /// trace. Wrong arity degrades trace quality; it does not break the call.
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
/// Expands to a `pub const MODULE: ModuleDesc`. One per subsystem crate, named in
/// `modules()` in `orbistoun-service`, which is the single list that hands them all to
/// the [`Registry`]. A crate that registered itself as well was a second copy of that
/// list, and the copies disagreed (D123).
#[macro_export]
macro_rules! guest_module {
    ($lib:literal { $($name:literal => $arity:literal),* $(,)? }) => {
        /// This crate's module description, consumed by the HLE registry.
        ///
        // `unreachable_pub` is allowed because the macro is also used inside
        // private modules (tests, and any subsystem that groups its libraries into
        // submodules). At a crate root - the normal case - the `pub` is real.
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
    /// What a stub **writes**, by symbol name.
    ///
    /// **The half a policy could not express.** An entry above says what a function answers;
    /// this says what it does, which for every wall this project has hit was the part that
    /// mattered - *"both current walls turned out to be a side effect nobody performed"*.
    ///
    /// The shape is the shape a sweep produces, so a measured `OutParameter` becomes an entry
    /// with no judgement in between and the loop can write one itself. Writing data is a thing
    /// it may do; writing code is not (D295).
    #[serde(default)]
    pub regions: HashMap<String, StubRegion>,
}

/// How a region reaches the guest.
///
/// **The two ways a function hands memory over**, and the only difference between them. A
/// contract that writes a base into an argument and one that returns it are the same
/// behaviour delivered differently, so they are one type with a field rather than two types
/// that cannot be compared (D300).
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
/// One write, of one base, into one argument - the shape a sweep can measure and nothing
/// wider. A policy that could express arbitrary side effects would be a program, and a
/// program in a data file is what principle 5 is trying to avoid rather than achieve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StubRegion {
    /// How the base reaches the guest.
    pub via: Delivery,
    /// How much space to reserve behind it.
    ///
    /// **Assumed, and deliberately a number in a file.** Nothing measured says how much the
    /// guest intends to use - a sweep sees where it faulted, not what it asked for. A value
    /// here can be changed and re-run without a rebuild, which is the entire reason policy is
    /// data (D291, D295).
    pub bytes: u64,
}

impl Default for StubPolicy {
    fn default() -> Self {
        // Unimplemented by default, not Ok: a silent success is how a wrong stub
        // becomes a hang forty thousand frames later. Make it loud, then relax
        // individual functions deliberately.
        Self {
            default_return: StubReturn::Unimplemented,
            overrides: HashMap::new(),
            regions: HashMap::new(),
        }
    }
}

impl StubPolicy {
    /// Folds policy the loop worked out into policy a person wrote.
    ///
    /// **A person's entry always wins**, and that is the whole safety property: nothing the
    /// loop writes can quietly override a deliberate choice, so letting it run unattended
    /// cannot cost a decision somebody made on purpose (D296).
    ///
    /// `default_return` is never taken from the learned side. It applies to every function
    /// with no entry of its own, so a loop that set it would be changing the behaviour of
    /// everything it had *not* measured - the opposite of what it earned.
    pub fn absorb(&mut self, learned: Self) {
        for (name, answer) in learned.overrides {
            self.overrides.entry(name).or_insert(answer);
        }
        for (name, region) in learned.regions {
            self.regions.entry(name).or_insert(region);
        }
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
    /// What a call will return until a real implementation lands.
    pub stub: StubReturn,
}

/// Everything orbistoun can answer, keyed by NID.
///
/// Built once at startup from every subsystem crate's `MODULE`, then queried by
/// the loader for each import a guest module names.
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
    /// **Exposed so a reader hashes names the same way.** A plain-name import is turned
    /// into a NID by hashing it, and hashing it with a different suffix from the one the
    /// registry resolves against produces a NID that matches nothing - silently, as an
    /// unresolved import rather than as an error (D305).
    #[must_use]
    pub const fn hasher(&self) -> &NidHasher {
        &self.hasher
    }

    /// Registers every import in `module`.
    ///
    /// Later registrations win on collision, which is what makes a real
    /// implementation able to displace a stub without unregistering it first.
    pub fn register(&mut self, module: ModuleDesc) {
        for import in module.imports {
            let nid = self.hasher.hash(import.name);
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
    /// `None` means the guest imported something never declared anywhere - the
    /// normal early-days case, and exactly what an import dump should report.
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
    use super::{Delivery, Registry, StubPolicy, StubRegion, StubReturn};
    use orbistoun_nid::NidHasher;
    use std::collections::HashMap;

    /// A person's entry always wins over one the loop worked out.
    ///
    /// **The safety property that makes unattended running acceptable.** Nothing a tier-one
    /// patcher writes can quietly override a deliberate choice, so the worst a wrong guess
    /// costs is a run - never a decision somebody made on purpose (D296).
    #[test]
    fn a_deliberate_entry_is_never_overridden_by_a_learned_one() {
        let mut mine = StubPolicy {
            default_return: StubReturn::Unimplemented,
            overrides: [("sceFoo".to_owned(), StubReturn::Raw(0x1234))]
                .into_iter()
                .collect(),
            regions: HashMap::new(),
        };
        let learned = StubPolicy {
            // Never taken: it applies to every function the loop did *not* measure, which is
            // the opposite of what it earned.
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

    #[test]
    fn declared_symbols_resolve_by_nid() {
        let hasher = NidHasher::new(*b"test-suffix");
        let r = registry(StubPolicy::default());

        let found = r.resolve(hasher.hash("testOpen")).expect("declared");
        assert_eq!(found.library, "libTest");
        assert_eq!(found.arity, 3);
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn undeclared_symbols_resolve_to_nothing() {
        let hasher = NidHasher::new(*b"test-suffix");
        let r = registry(StubPolicy::default());
        assert!(r.resolve(hasher.hash("testNotDeclared")).is_none());
    }

    #[test]
    fn default_policy_is_loud_not_silent_success() {
        // The whole argument for this default: a stub that reports success is
        // indistinguishable from working code until much later.
        let r = registry(StubPolicy::default());
        let hasher = NidHasher::new(*b"test-suffix");
        let found = r.resolve(hasher.hash("testInit")).expect("declared");
        assert_eq!(found.stub, StubReturn::Unimplemented);
    }

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
        // The override must not leak to its neighbours - this is the bisection
        // workflow's core requirement.
        assert_eq!(
            r.resolve(hasher.hash("testOpen")).expect("declared").stub,
            StubReturn::Unimplemented
        );
    }
}
