//! Enumerating what orbistoun declares.

use orbistoun_hle::ModuleDesc;
use orbistoun_nid::NidHasher;
use serde::{Deserialize, Serialize};

/// One function orbistoun declares, with the hash it would be imported by.
///
/// `Ord` so output can be sorted deterministically - reports are diffed between runs,
/// and ordering churn would read as change.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeclaredSymbol {
    /// Library the symbol belongs to.
    pub library: String,
    /// Symbol name, exactly as the firmware exports it.
    pub symbol: String,
    /// Hash a guest module would import it by.
    pub nid: u64,
    /// Integer argument count. Provisional across the subsystem crates - affects
    /// trace fidelity, not whether a call works.
    pub arity: u8,
    /// Whether a real handler is attached, as opposed to a stub answering the policy.
    ///
    /// **Counted here so nobody counts it by hand.** The implemented total was quoted in
    /// three documents and derived each time by reading the `implementations()` lists,
    /// which is how it came to be wrong by one for a while - a multi-line entry does not
    /// look like the others (D199).
    pub implemented: bool,
}

/// Every module the service registers.
///
/// **The single list, and it has to stay that way.** A second copy existed - the service
/// hand-called `register` per crate - and adding `libc` to only one of them produced a
/// function that `orbistoun-cli symbols` listed, that a trace named correctly, and that
/// resolved to nothing. Every layer agreed except the one that mattered (D123).
pub(crate) fn modules() -> [ModuleDesc; 12] {
    [
        orbistoun_kernel::MODULE,
        orbistoun_kernel::ult::MODULE,
        orbistoun_libc::MODULE,
        orbistoun_posix::MODULE,
        orbistoun_gpu::MODULE,
        orbistoun_audio::MODULE,
        orbistoun_video::MODULE,
        orbistoun_input::MODULE,
        orbistoun_fs::MODULE,
        orbistoun_systemservice::MODULE,
        orbistoun_systemservice::user::MODULE,
        orbistoun_systemservice::sysmodule::MODULE,
    ]
}

/// Every implementation the subsystem crates provide, by symbol name.
///
/// The counterpart to [`modules`], and kept beside it for the same reason: a function
/// declared in one place and implemented in another drifts apart silently, and the
/// failure mode is code that looks written and never runs.
pub(crate) fn implementations() -> Vec<(&'static str, orbistoun_core::GuestFn)> {
    let mut all = orbistoun_kernel::implementations().to_vec();
    all.extend(orbistoun_libc::implementations());
    all.extend(orbistoun_posix::implementations());
    all.extend_from_slice(orbistoun_video::implementations());
    all.extend_from_slice(orbistoun_gpu::implementations());
    all.extend_from_slice(orbistoun_fs::implementations());
    all.extend_from_slice(orbistoun_input::implementations());
    all.extend_from_slice(orbistoun_audio::implementations());
    all.extend_from_slice(orbistoun_systemservice::implementations());
    all
}

/// Every implementation that speaks in floating-point registers.
///
/// Only libc has any: the maths library is defined by IEEE 754 rather than by the
/// platform, and nothing else declared here takes or returns a `double` (D268).
pub(crate) fn float_implementations() -> Vec<(&'static str, orbistoun_core::GuestFloatFn)> {
    orbistoun_libc::math::implementations().to_vec()
}

/// One implementation a run-time lookup may answer with.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Resolvable {
    /// Answers in `rax`.
    Integer(orbistoun_core::GuestFn),
    /// Answers in `xmm0`.
    Float(orbistoun_core::GuestFloatFn),
}

/// Every implementation, in the order the by-name stubs are laid out in.
///
/// **One list, so two places cannot disagree about which slot is which.** The stub table
/// binds a handler to slot `imports + n`, and the call trace labels that same slot with a
/// name; if either walked its own list, a resolved call would be attributed to a different
/// function than the one that ran - which is worse than no label at all (D366).
pub(crate) fn resolvable() -> Vec<(&'static str, Resolvable)> {
    let mut all: Vec<(&'static str, Resolvable)> = implementations()
        .into_iter()
        .map(|(name, f)| (name, Resolvable::Integer(f)))
        .collect();
    all.extend(
        float_implementations()
            .into_iter()
            .map(|(name, f)| (name, Resolvable::Float(f))),
    );
    all
}

/// Names a syscall goes by that are not simply the number's name without its prefix.
///
/// **A table rather than a rule with exceptions.** The rule - `SYS_write` is `write` - covers
/// almost all of them, and "almost" is the problem: a rule applied by code to the ones it does
/// not cover binds a number to the wrong function silently, which is the worst failure this
/// boundary has (D378).
const SPELT_DIFFERENTLY: &[(&str, &str)] = &[
    // FreeBSD's own underscored spelling for the call `sysctl(3)` wraps.
    ("SYS___sysctl", "sysctl"),
    // The two exits. `SYS_exit` is the process one; the thread one has no implementation
    // here and is deliberately absent rather than bound to the process exit.
    ("SYS_exit", "exit"),
];

/// Numbers this must not bind even though the name matches.
///
/// `SYS_syscall` and `SYS___syscall` are the indirect forms: the number they carry is *another*
/// number, in the first argument. Binding them to anything called `syscall` would perform the
/// wrong call with the arguments shifted by one.
const NOT_A_CALL: &[&str] = &["SYS_syscall", "SYS___syscall"];

/// What each syscall number performs, for the numbers something here implements.
///
/// # A number is a name the guest did not spell
///
/// `SYS_write` is four and `write` has been implemented for a while. The mapping between them
/// is harvested from `sys/sys/syscall.h` rather than written out, so the numbers stay traceable
/// to the header the way every other constant here is (D378).
///
/// A number whose name nothing implements is simply absent, and the dispatcher answers those
/// the way a kernel does.
pub(crate) fn syscalls() -> std::collections::BTreeMap<u64, (&'static str, orbistoun_core::GuestFn)>
{
    let implemented: std::collections::BTreeMap<&'static str, orbistoun_core::GuestFn> =
        implementations().into_iter().collect();
    let renamed: std::collections::BTreeMap<&str, &str> =
        SPELT_DIFFERENTLY.iter().copied().collect();

    let mut out = std::collections::BTreeMap::new();
    // The target's own numbers first, then FreeBSD's. Kept in separate files because one is
    // generated from headers and the other is a record of what guests asked for (D403).
    let declared = orbistoun_hle::constants::vendor_constants_in("syscall")
        .into_iter()
        .chain(orbistoun_hle::constants::abi_constants_in("syscall"));
    for (constant, number) in declared {
        if NOT_A_CALL.contains(&constant.as_str()) {
            continue;
        }
        let Ok(number) = u64::try_from(number) else {
            continue;
        };
        let called = renamed
            .get(constant.as_str())
            .copied()
            .or_else(|| constant.strip_prefix("SYS_"))
            .unwrap_or_default();
        if let Some((name, function)) = implemented.get_key_value(called) {
            out.insert(number, (*name, *function));
        }
    }
    out
}

/// What the dispatcher answers for a number nothing implements.
///
/// `ENOSYS` negated, which is how a FreeBSD syscall reports failure to the stub that called
/// it. Harvested, so it stays traceable - and a fallback that is still a detectable failure
/// when the table cannot be read at all.
pub(crate) fn syscall_refusal() -> u64 {
    let enosys = orbistoun_hle::constants::abi_constant("errno", "ENOSYS").unwrap_or(1);
    (-enosys) as u64
}

/// Every declared symbol, sorted.
pub(crate) fn all(hasher: &NidHasher) -> Vec<DeclaredSymbol> {
    // Both tables. A function that answers in `xmm0` is as implemented as one that answers
    // in `rax`, and counting only the first would report the maths library as missing while
    // it worked (D268).
    let attached: std::collections::BTreeSet<&str> = implementations()
        .into_iter()
        .map(|(name, _)| name)
        .chain(float_implementations().into_iter().map(|(name, _)| name))
        .collect();
    let mut out: Vec<DeclaredSymbol> = modules()
        .into_iter()
        .flat_map(|m| {
            let attached = &attached;
            m.imports.iter().map(move |i| DeclaredSymbol {
                library: m.name.to_owned(),
                symbol: i.name.to_owned(),
                nid: hasher.hash(i.name).as_raw(),
                arity: i.arity,
                implemented: attached.contains(i.name),
            })
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::{all, modules};
    use orbistoun_nid::NidHasher;

    /// **The list the stub table and the call trace both walk, walked twice** (D366).
    ///
    /// The binding says "slot `imports + n` is `resolvable()[n]`" and the label says the
    /// same thing, in a different function. If the order were not stable, a call resolved
    /// at run time would be attributed to a different function than the one that ran -
    /// which is worse than no label, because it reads as evidence.
    #[test]
    fn the_resolvable_list_is_the_same_list_every_time_it_is_asked_for() {
        let first: Vec<&str> = super::resolvable().into_iter().map(|(n, _)| n).collect();
        let second: Vec<&str> = super::resolvable().into_iter().map(|(n, _)| n).collect();
        assert_eq!(first, second);
        assert!(
            !first.is_empty(),
            "and it is not empty, which would pass vacuously"
        );
    }

    /// Everything implemented is reachable by name, not only by import.
    ///
    /// The payloads resolve most of their C library at run time rather than importing it,
    /// so a function that exists but cannot be *found* by name is a function they cannot
    /// call (D365).
    #[test]
    fn every_implementation_can_be_resolved_by_name() {
        let reachable: std::collections::BTreeSet<&str> =
            super::resolvable().into_iter().map(|(n, _)| n).collect();
        for (name, _) in super::implementations() {
            assert!(
                reachable.contains(name),
                "{name} cannot be looked up by name"
            );
        }
        for (name, _) in super::float_implementations() {
            assert!(
                reachable.contains(name),
                "{name} cannot be looked up by name"
            );
        }
    }

    /// The resolver a payload asks for first is one of the things it can resolve.
    ///
    /// A runtime's opening move is to look up the resolver itself through the structure it
    /// was handed (D365); an emulator that answers the call but cannot answer that name has
    /// stopped it at the first step.
    #[test]
    fn the_resolver_can_resolve_itself() {
        let reachable: std::collections::BTreeSet<&str> =
            super::resolvable().into_iter().map(|(n, _)| n).collect();
        assert!(reachable.contains("sceKernelDlsym"));
    }

    /// **The mapping is harvested, and it really binds things** (D378).
    ///
    /// A rule that produced an empty table would pass every other check here in silence, so
    /// this asserts both that the well-known numbers are bound and that they are bound to the
    /// right names - `write` is four on this platform and nothing else may claim four.
    #[test]
    fn the_syscall_table_binds_the_numbers_the_header_gives() {
        let table = super::syscalls();
        assert!(
            table.len() > 10,
            "a table with almost nothing in it is a bug"
        );

        for (name, number) in [("read", 3), ("write", 4), ("open", 5), ("close", 6)] {
            let bound = table
                .get(&number)
                .unwrap_or_else(|| panic!("{name} is {number}"));
            assert_eq!(bound.0, name, "{number} must perform {name}");
        }
    }

    /// The indirect forms are refused rather than bound to something plausible.
    ///
    /// `SYS_syscall` carries *another* number in its first argument. Binding it to anything
    /// would perform the wrong call with every argument shifted by one.
    #[test]
    fn the_indirect_syscall_forms_are_not_bound() {
        let table = super::syscalls();
        let indirect = orbistoun_hle::constants::abi_constant("syscall", "SYS_syscall")
            .and_then(|n| u64::try_from(n).ok())
            .expect("harvested");
        assert!(!table.contains_key(&indirect));
    }

    /// An unknown number's answer is the header's `ENOSYS`, not a made-up one.
    #[test]
    fn the_refusal_is_the_headers_number() {
        let enosys = orbistoun_hle::constants::abi_constant("errno", "ENOSYS").expect("harvested");
        assert_eq!(super::syscall_refusal(), (-enosys) as u64);
        assert!((super::syscall_refusal() as i64) < 0, "and it is a failure");
    }

    #[test]
    fn no_symbol_is_declared_twice() {
        // A duplicate would mean two subsystems claim the same function, and the
        // registry's last-wins rule would silently pick one.
        let symbols = all(&NidHasher::new(*b"x"));
        let mut seen = std::collections::BTreeSet::new();
        for s in &symbols {
            assert!(
                seen.insert(s.symbol.clone()),
                "{} declared more than once",
                s.symbol
            );
        }
    }

    /// Everything implemented is reachable through the registry a **run** builds.
    ///
    /// # Why here, and not in the crate that implements it
    ///
    /// A subsystem crate can only test the registry it builds itself, and no run builds
    /// that one - `modules()` is the single list (D123), and the per-crate `register`
    /// functions left over from the design D123 replaced are called by nothing. A test
    /// there passes or fails against a registry that never runs, which is the same shape as
    /// the bug: agreeing at every layer except the one that matters (D281).
    ///
    /// **Both tables.** A function answering in `xmm0` is as implemented as one answering
    /// in `rax`, and checking only the integer one would report the maths library as
    /// unreachable while it worked, or the reverse (D268).
    #[test]
    fn every_implementation_resolves_through_the_registry_a_run_builds() {
        use orbistoun_hle::{Registry, StubPolicy};

        let hasher = NidHasher::new(*b"registry-reachability");
        let mut registry = Registry::new(hasher.clone(), StubPolicy::default());
        for module in modules() {
            registry.register(module);
        }

        let integer = super::implementations().into_iter().map(|(name, _)| name);
        let floating = super::float_implementations()
            .into_iter()
            .map(|(name, _)| name);
        for name in integer.chain(floating) {
            assert!(
                registry.resolve(hasher.hash(name)).is_some(),
                "{name} is implemented, but no module declaring it reaches the registry"
            );
        }
    }

    #[test]
    fn every_module_contributes_at_least_one_symbol() {
        for m in modules() {
            assert!(!m.imports.is_empty(), "{} declares nothing", m.name);
        }
    }

    #[test]
    fn nids_differ_per_symbol() {
        // A collision here would make two functions indistinguishable at resolution
        // time, which is a silent wrong-function-called bug.
        let symbols = all(&NidHasher::new(*b"x"));
        let mut seen = std::collections::BTreeSet::new();
        for s in &symbols {
            assert!(seen.insert(s.nid), "NID collision on {}", s.symbol);
        }
    }
}

#[cfg(test)]
mod knowledge_tests {
    use orbistoun_hle::knowledge::Knowledge;

    /// Libraries that declare functions and serve none, **exactly** - each with its reason.
    ///
    /// Not "some libraries are unimplemented", which drifts into meaninglessness. The exact
    /// set, so that a library gaining an implementation fails until its entry is deleted,
    /// and one **losing its registration fails until an entry is added and justified**. Both
    /// directions are load-bearing (`docs/TESTING.md`).
    // Empty, and that is the goal: every declared library now answers at least one call. Two
    // entries retired together - `libSceGnmDriver` once translated its command streams entirely
    // below the shim, but the dispatch builders (D427) answer calls here now; `libSceAudioOut` once
    // implemented nothing rather than fake sound, and still implements no *output*, but its init now
    // succeeds honestly (setting a subsystem up is not claiming a sound was made). A module that
    // genuinely serves nothing goes back here with its reason.
    const SERVES_NOTHING: &[(&str, &str)] = &[];

    /// **A declared library that serves nothing is either listed above or a bug.**
    ///
    /// The guard that would have caught `orbistoun-input`: its module was registered in
    /// `modules()` and its `implementations()` were never added to the list below it, so six
    /// functions were declared, resolvable, and answered by nobody. Nothing failed, because
    /// the test that checks implementations iterates over *the list they were missing from* -
    /// the vacuous-loop failure `docs/TESTING.md` describes, arriving somewhere new.
    #[test]
    fn every_declared_library_either_serves_something_or_says_why_not() {
        let attached: std::collections::BTreeSet<&str> = super::implementations()
            .into_iter()
            .map(|(name, _)| name)
            .chain(super::float_implementations().into_iter().map(|(n, _)| n))
            .collect();

        for module in super::modules() {
            let serves = module
                .imports
                .iter()
                .filter(|import| attached.contains(import.name))
                .count();
            let excused = SERVES_NOTHING.iter().any(|(name, _)| *name == module.name);

            assert!(
                serves > 0 || excused,
                concat!(
                    "{} declares {} function(s) and serves none. Either its implementations are ",
                    "not registered in symbols::implementations(), or it belongs in ",
                    "SERVES_NOTHING with a reason."
                ),
                module.name,
                module.imports.len()
            );
            assert!(
                !(serves > 0 && excused),
                // Named, because `concat!` is a macro call rather than a literal and implicit
                // capture only works on a literal (D362).
                concat!(
                    "{} serves {serves} function(s) but is still listed as serving nothing - ",
                    "delete its entry from SERVES_NOTHING"
                ),
                module.name,
                serves = serves
            );
        }
    }

    /// Every excuse names a library that is actually declared.
    ///
    /// The other direction: a renamed or deleted module would leave an entry excusing
    /// something that no longer exists, and a list nobody prunes stops being a statement.
    #[test]
    fn every_excuse_belongs_to_a_library_that_exists() {
        let declared: std::collections::BTreeSet<&str> =
            super::modules().into_iter().map(|m| m.name).collect();

        for (name, _) in SERVES_NOTHING {
            assert!(
                declared.contains(name),
                "{name} is excused from serving anything, but no module declares it"
            );
        }
    }

    #[test]
    fn declared_arity_and_recorded_arity_never_disagree() {
        // Two places hold an arity: the `guest_module!` declaration the code compiles
        // against, and the knowledge file a person reads. They are allowed to be
        // incomplete - a function can be declared without being understood - but they
        // must never *contradict*, because a reader has no way to tell which is stale
        // and a trace would render arguments the implementation does not take (D122).
        let knowledge = Knowledge::builtin();
        for module in super::modules() {
            for import in module.imports {
                let Some(known) = knowledge.get(import.name) else {
                    continue;
                };
                let Some(arity) = known.arity else {
                    continue;
                };
                assert_eq!(
                    arity, import.arity,
                    "{} is declared with arity {} and recorded with {arity}",
                    import.name, import.arity
                );
            }
        }
    }

    #[test]
    fn every_implemented_function_is_written_down() {
        // Implementing something without recording what was learned is how the knowledge
        // ends up existing only in a conversation, which is the failure this file exists
        // to prevent.
        let knowledge = Knowledge::builtin();
        for (name, _) in super::implementations() {
            assert!(
                knowledge.get(name).is_some(),
                "{name} is implemented but nothing is recorded about it"
            );
        }
    }

    #[test]
    fn a_recorded_argument_list_matches_the_recorded_arity() {
        // Listing three arguments for a four-argument function is the kind of internal
        // contradiction that makes a reader distrust the whole file.
        for f in Knowledge::builtin().functions() {
            let (Some(arity), false) = (f.arity, f.arguments.is_empty()) else {
                continue;
            };
            assert_eq!(
                f.arguments.len(),
                arity as usize,
                "{} records {} arguments but arity {arity}",
                f.name,
                f.arguments.len()
            );
        }
    }
}
