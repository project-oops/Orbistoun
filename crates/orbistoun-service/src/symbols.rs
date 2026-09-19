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
pub(crate) fn modules() -> [ModuleDesc; 42] {
    [
        orbistoun_kernel::MODULE,
        orbistoun_kernel::ult::MODULE,
        orbistoun_kernel::sync_on_address::MODULE,
        orbistoun_libc::MODULE,
        orbistoun_posix::MODULE,
        orbistoun_gpu::MODULE,
        orbistoun_gpu::agc::MODULE,
        orbistoun_gpu::agc_driver::MODULE,
        orbistoun_audio::MODULE,
        orbistoun_video::MODULE,
        orbistoun_input::MODULE,
        orbistoun_fs::MODULE,
        orbistoun_systemservice::MODULE,
        orbistoun_systemservice::user::MODULE,
        orbistoun_systemservice::sysmodule::MODULE,
        orbistoun_audio::ajm::MODULE,
        orbistoun_audio::audio3d::MODULE,
        orbistoun_audio::audio_in::MODULE,
        orbistoun_audio::audio_out2::MODULE,
        orbistoun_gpu::ampr::MODULE,
        orbistoun_input::ime::MODULE,
        orbistoun_input::ime_dialog::MODULE,
        orbistoun_input::keyboard::MODULE,
        orbistoun_input::mouse::MODULE,
        orbistoun_net::http::MODULE,
        orbistoun_net::http2::MODULE,
        orbistoun_net::netctl::MODULE,
        orbistoun_net::npmanager::MODULE,
        orbistoun_net::npwebapi2::MODULE,
        orbistoun_net::socket::MODULE,
        orbistoun_net::ssl::MODULE,
        orbistoun_systemservice::app_content::MODULE,
        orbistoun_systemservice::common_dialog::MODULE,
        orbistoun_systemservice::coredump::MODULE,
        orbistoun_systemservice::error_dialog::MODULE,
        orbistoun_systemservice::json2::MODULE,
        orbistoun_systemservice::msg_dialog::MODULE,
        orbistoun_systemservice::remoteplay::MODULE,
        orbistoun_systemservice::save_data::MODULE,
        orbistoun_systemservice::web_browser_dialog::MODULE,
        orbistoun_video::av_player::MODULE,
        orbistoun_video::recording::MODULE,
    ]
}

/// Every implementation the subsystem crates provide, by symbol name.
///
/// The counterpart to `modules`, and kept beside it for the same reason: a function
/// declared in one place and implemented in another drifts apart silently, and the
/// failure mode is code that looks written and never runs.
/// The implementation a guest would reach by importing `name`.
///
/// **Exposed so something can call one without a guest.** Every other route into these
/// functions goes through a loaded image, a thunk table and a relocation, which is a great
/// deal of machinery to stand up in order to ask what one function answers. A harness
/// checking orbistoun against a measured console value needs exactly this, and so does a
/// differential run against another implementation of the same interface.
///
/// The arguments are plain words and guest memory is the host's under an identity mapping, so
/// a caller passes the address of its own storage and the implementation writes through it -
/// which is what a guest does.
#[must_use]
pub fn implementation_named(name: &str) -> Option<orbistoun_core::GuestFn> {
    implementations()
        .into_iter()
        .find(|(known, _)| *known == name)
        .map(|(_, f)| f)
}

/// The implementation of `name` that answers in a floating-point register.
///
/// **A separate door because they are a separate table.** A function answers in `rax` or in
/// `xmm0` and never both (D268), so a harness looking one up by name has to know which it
/// wants - and `strtod` reading an integer argument while answering a float is exactly the
/// case that makes the distinction visible rather than academic.
#[must_use]
pub fn float_implementation_named(name: &str) -> Option<orbistoun_core::GuestFloatFn> {
    float_implementations()
        .into_iter()
        .find(|(known, _)| *known == name)
        .map(|(_, f)| f)
}

pub(crate) fn implementations() -> Vec<(&'static str, orbistoun_core::GuestFn)> {
    let mut all = orbistoun_kernel::implementations().to_vec();
    all.extend(orbistoun_libc::implementations());
    all.extend(orbistoun_posix::implementations());
    all.extend_from_slice(orbistoun_video::implementations());
    all.extend_from_slice(orbistoun_gpu::implementations());
    // libSceAgc's own implementations, declared beside the Gnm builders rather than folded in:
    // `sceAgcCreateShader` fills a guest-adjacent shader object from the measured 3c5e model (D556),
    // and the shader-linkage calls (interpolant mapping, prim state, link shaders) from 9a41.
    all.extend_from_slice(orbistoun_gpu::agc::implementations());
    // libSceAgcDriver: `sceAgcDriverCreateQueue` accepts the Type 0/3 queue and returns success (9a41).
    all.extend_from_slice(orbistoun_gpu::agc_driver::implementations());
    all.extend_from_slice(orbistoun_fs::implementations());
    // The socket calls in their vendor spelling. The bodies are `orbistoun-fs`'s; this crate
    // declares `libSceNet` and encodes a failure the way that library numbers one (D667).
    all.extend_from_slice(orbistoun_net::implementations());
    all.extend_from_slice(orbistoun_input::implementations());
    // The mouse, registered beside the pad rather than folded into it: the crate root answers a
    // `&'static` slice, so gathering two modules there would mean allocating, and one explicit
    // line is cheaper than that and easier to notice when a third arrives (D673).
    all.extend_from_slice(orbistoun_input::mouse::implementations());
    all.extend_from_slice(orbistoun_audio::implementations());
    all.extend_from_slice(orbistoun_systemservice::implementations());
    // libSceCommonDialog: `sceCommonDialogInitialize` answers 0, the guest-observed init the three
    // retail Unity titles need to get past their first wall (D678); registered here beside its
    // declaration rather than folded into the crate root's slice, as the Agc modules are.
    all.extend_from_slice(orbistoun_systemservice::common_dialog::implementations());
    // libSceAppContent: the app-content init sequence a Unity IL2CPP title runs at startup -
    // `sceAppContentInitialize` (guest-observed 0) and `sceAppContentAppParamGetInt` (placeholder
    // int, as sceSystemServiceParamGetInt), which PPSA25872's libil2cpp fails on otherwise (D680).
    all.extend_from_slice(orbistoun_systemservice::app_content::implementations());
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
    // **The two exits, and neither needs an entry here.** The process one is spelt `SYS__exit`
    // in FreeBSD's table - entry 1 is the raw `_exit`, not the `exit(3)` wrapper - so stripping
    // `SYS_` already yields the name `orbistoun-libc` answers to. An entry reading `SYS_exit`
    // stood here and matched nothing, because no harvested constant is called that: the mapping
    // was dead, and with it syscall 1, which a guest asks for at the end of every clean run.
    // The thread one, `SYS_thr_exit` (431), has no implementation and is deliberately absent
    // rather than bound to the process exit.
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

        for (name, number) in [
            ("read", 3),
            ("write", 4),
            ("open", 5),
            ("close", 6),
            // Entry 1 is the raw `_exit`, not the `exit(3)` wrapper. It bound to nothing at all
            // until worklog 543, which is why it is pinned here beside the others.
            ("_exit", 1),
        ] {
            let bound = table
                .get(&number)
                .unwrap_or_else(|| panic!("{name} is {number}"));
            assert_eq!(bound.0, name, "{number} must perform {name}");
        }
    }

    /// **Every rename names a constant that exists.**
    ///
    /// `SPELT_DIFFERENTLY` maps a harvested constant to the name something answers to, and a key
    /// that matches no constant is silently inert - the entry looks like a binding, reads like one
    /// in review, and does nothing. One stood here for a long time: `SYS_exit`, which no harvested
    /// constant is called, because FreeBSD's entry 1 is the raw `_exit`. The result was that
    /// syscall 1 - what a guest calls at the end of every clean run - reached no implementation,
    /// while an implementation for it existed the whole time.
    ///
    /// This asserts the shape rather than that one case, because the failure is invisible by
    /// construction: nothing breaks, a number just quietly answers `ENOSYS` forever.
    #[test]
    fn every_rename_names_a_constant_that_exists() {
        let declared: std::collections::BTreeSet<String> =
            orbistoun_hle::constants::vendor_constants_in("syscall")
                .into_iter()
                .chain(orbistoun_hle::constants::abi_constants_in("syscall"))
                .map(|(name, _)| name)
                .collect();
        assert!(
            !declared.is_empty(),
            "no syscall constants were read at all"
        );

        for (constant, _) in super::SPELT_DIFFERENTLY {
            assert!(
                declared.contains(*constant),
                "{constant} is renamed here but no harvested constant is called that - it is dead"
            );
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
    // Twenty-three libraries serve nothing today, each with its reason below - the README's generated
    // block reports the same, `149 across 23 libraries`. Two entries retired *from* here together -
    // `libSceGnmDriver` once translated its command streams entirely
    // below the shim, but the dispatch builders (D427) answer calls here now; `libSceAudioOut` once
    // implemented nothing rather than fake sound, and still implements no *output*, but its init now
    // succeeds honestly (setting a subsystem up is not claiming a sound was made). A module that
    // genuinely serves nothing goes back here with its reason.
    const SERVES_NOTHING: &[(&str, &str)] = &[
        (
            "libSceAjm",
            "declared as 14 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceAudio3d",
            "declared as 7 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceAudioIn",
            "declared as 3 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceAudioOut2",
            "declared as 13 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceAmpr",
            "declared as 5 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceIme",
            "declared as 5 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceImeDialog",
            "declared as 5 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceKeyboard",
            "declared as 3 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceHttp",
            "declared as 1 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceHttp2",
            "declared as 23 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceNetCtl",
            "declared as 2 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceNpManager",
            "declared as 4 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceNpWebApi2",
            "declared as 8 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceSsl",
            "declared as 10 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceCoredump",
            "declared as 1 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceErrorDialog",
            "declared as 1 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceJson2",
            "declared as 3 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceMsgDialog.native",
            "declared as 4 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceRemoteplay",
            "declared as 2 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceSaveData_native",
            "declared as 5 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceWebBrowserDialog",
            "declared as 4 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceAvPlayer",
            "declared as 22 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
        (
            "libSceVideoRecording",
            "declared as 4 name(s) and nothing else, read out of a real import table. Nothing is implemented: what the declaration buys is that a guest reaching this interface is named and counted rather than dying on an unresolved import (D505).",
        ),
    ];

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

    /// **An excuse that counts the names must count them right.**
    ///
    /// Every reason above opens `declared as N name(s)`, and nothing checked N. It had
    /// already rotted: `libSceVideoRecording` said ten while its module declares four,
    /// because six names were taken out of that module for having no provenance - they came
    /// from a probe check that was read as measuring that they *resolve* and in fact records
    /// the branch where the lookup returned null. The removal was right and the excuse was
    /// not updated with it.
    ///
    /// Two other numbers describe this same set and **neither one was wrong**, which is why
    /// the drift was invisible: the guard above checks membership rather than size, and
    /// README's `149 across 23 libraries` is counted from the declarations themselves in
    /// `orbistoun-cli`, so it never read this string at all. A transcribed number with two
    /// correct derived neighbours is the easiest kind to leave rotting (D085's rule, one
    /// level down: if it can be derived, do not also write it down unchecked).
    ///
    /// Tolerant on purpose: a reason that does not open with that phrase is a bespoke one -
    /// the retired entries had them - and has nothing to check.
    #[test]
    fn an_excuse_that_states_a_name_count_states_the_right_one() {
        let declared: std::collections::BTreeMap<&str, usize> = super::modules()
            .into_iter()
            .map(|m| (m.name, m.imports.len()))
            .collect();

        let mut checked = 0_usize;
        for (name, reason) in SERVES_NOTHING {
            let Some(rest) = reason.strip_prefix("declared as ") else {
                continue;
            };
            let Some((digits, _)) = rest.split_once(" name(s)") else {
                continue;
            };
            let claimed: usize = digits
                .parse()
                .unwrap_or_else(|_| panic!("{name} opens `declared as {digits} name(s)`, which is the counted form, but {digits} is not a number"));
            let actual = declared
                .get(name)
                .copied()
                .unwrap_or_else(|| panic!("{name} is excused but no module declares it"));
            assert_eq!(
                claimed,
                actual,
                concat!(
                    "{} says it declares {claimed} name(s) and its module declares {actual}. ",
                    "The module is the fact; this sentence is a copy of it."
                ),
                name,
                claimed = claimed,
                actual = actual
            );
            checked += 1;
        }

        // **The count of what was checked, asserted.** Every entry uses the counted form
        // today, so a loop that silently checked none of them - a `strip_prefix` that stopped
        // matching after a rewording, say - would pass exactly as loudly as one that checked
        // all of them. That is the vacuous-loop failure `docs/TESTING.md` names.
        assert_eq!(
            checked,
            SERVES_NOTHING.len(),
            "every entry states a name count today, so all of them should have been checked"
        );
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
