//! Enumerating what orbistoun declares.

use orbistoun_hle::ModuleDesc;
use orbistoun_nid::NidHasher;
use serde::{Deserialize, Serialize};

/// One function orbistoun declares, with the hash it would be imported by.
///
/// `Ord` so output sorts deterministically: reports are diffed between runs, and ordering churn
/// would read as change.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeclaredSymbol {
    /// Library the symbol belongs to.
    pub library: String,
    /// Symbol name, exactly as the platform exports it.
    pub symbol: String,
    /// Hash a guest module would import it by.
    pub nid: u64,
    /// Integer argument count. Affects trace fidelity, not whether a call works.
    pub arity: u8,
    /// Whether a real handler is attached, as opposed to a stub answering the policy. Counted here
    /// so nobody derives the total by reading `implementations()` lists by hand.
    pub implemented: bool,
}

/// Every module the service registers.
///
/// The single list (D123): a second, hand-maintained registration path lets a function be listed,
/// named in traces, and resolve to nothing.
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

/// The implementation a guest would reach by importing `name`.
///
/// Exposed so a harness or a differential run can call one function without a loaded image, a thunk
/// table and a relocation. Arguments are plain words, and guest memory is the host's, so a caller
/// passes the address of its own storage and the implementation writes through it, as a guest's
/// would be.
#[must_use]
pub fn implementation_named(name: &str) -> Option<orbistoun_core::GuestFn> {
    implementations()
        .into_iter()
        .find(|(known, _)| *known == name)
        .map(|(_, f)| f)
}

/// The implementation of `name` that answers in a floating-point register.
///
/// A separate table: a function answers in `rax` or in `xmm0` and never both (D268), so a harness
/// looking one up by name has to say which it wants.
#[must_use]
pub fn float_implementation_named(name: &str) -> Option<orbistoun_core::GuestFloatFn> {
    float_implementations()
        .into_iter()
        .find(|(known, _)| *known == name)
        .map(|(_, f)| f)
}

/// Every implementation the subsystem crates provide, by symbol name. Kept beside `modules` so
/// declaration and implementation cannot drift apart silently.
pub(crate) fn implementations() -> Vec<(&'static str, orbistoun_core::GuestFn)> {
    let mut all = orbistoun_kernel::implementations().to_vec();
    all.extend(orbistoun_libc::implementations());
    all.extend(orbistoun_posix::implementations());
    all.extend_from_slice(orbistoun_video::implementations());
    all.extend_from_slice(orbistoun_gpu::implementations());
    // libSceAgc's own implementations, declared beside the command-buffer builders:
    // `sceAgcCreateShader` fills a guest-adjacent shader object, and the shader-linkage calls
    // (interpolant mapping, prim state, link shaders).
    all.extend_from_slice(orbistoun_gpu::agc::implementations());
    // libSceAgcDriver: `sceAgcDriverCreateQueue` accepts the Type 0/3 queue and returns success.
    all.extend_from_slice(orbistoun_gpu::agc_driver::implementations());
    all.extend_from_slice(orbistoun_fs::implementations());
    // Reading a directory through its descriptor, by the system-call names.
    all.extend_from_slice(orbistoun_fs::dirent::implementations());
    // The socket calls in their vendor spelling. The bodies are `orbistoun-fs`'s; this crate
    // declares `libSceNet` and encodes a failure the way that library numbers one (D667).
    all.extend_from_slice(orbistoun_net::implementations());
    all.extend_from_slice(orbistoun_input::implementations());
    // The mouse, registered beside the pad: the crate root answers a `&'static` slice, so gathering
    // two modules there would allocate.
    all.extend_from_slice(orbistoun_input::mouse::implementations());
    all.extend_from_slice(orbistoun_audio::implementations());
    all.extend_from_slice(orbistoun_systemservice::implementations());
    // A launcher's request to start another title, handed to whoever owns the run.
    all.extend_from_slice(orbistoun_systemservice::launch::implementations());
    // libSceCommonDialog: `sceCommonDialogInitialize` answers 0, as the guest observes on hardware.
    // Registered beside its declaration, as the Agc modules are.
    all.extend_from_slice(orbistoun_systemservice::common_dialog::implementations());
    // libSceAppContent: the app-content init sequence an IL2CPP title runs at startup,
    // `sceAppContentInitialize` (0) and `sceAppContentAppParamGetInt` (a placeholder integer, as
    // `sceSystemServiceParamGetInt`).
    all.extend_from_slice(orbistoun_systemservice::app_content::implementations());
    // libSceCoredump: a crash-handler registration, accepted with 0. Orbistoun catches guest faults
    // itself and never invokes the handler, but the registration succeeds rather than answering the
    // placeholder a caller reads as failure.
    all.extend_from_slice(orbistoun_systemservice::coredump::implementations());
    all
}

/// Every implementation that speaks in floating-point registers.
///
/// Only libc has any: the maths library is defined by IEEE 754, and nothing else declared here
/// takes or returns a `double` (D268).
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
/// One list, so the stub table binding slot `imports + n` and the call trace labelling that slot
/// agree; otherwise a resolved call would be attributed to a different function (D366).
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

/// Names a syscall goes by that are not the number's name without its `SYS_` prefix.
///
/// A table rather than a rule with exceptions, because a rule applied to a name it does not cover
/// binds a number to the wrong function silently (D378).
const SPELT_DIFFERENTLY: &[(&str, &str)] = &[
    // FreeBSD's own underscored spelling for the call `sysctl(3)` wraps.
    ("SYS___sysctl", "sysctl"),
    // The vendor kernel-log write (601), served by the same function as `sceKernelDebugOutText`.
    // Both write the string at the second argument to the operator log and ignore the first (the
    // channel, or the operation selector, `7` in every observed use). A guest that logs by raw
    // number, as the obSCEne runtime does, is served rather than answered `ENOSYS`.
    ("SYS_vendor_klog", "sceKernelDebugOutText"),
    // The FreeBSD 11 directory reads, served by the vendor names that write the same records: a
    // launcher lists `/user/app` by the raw numbers 196 and 272.
    ("SYS_freebsd11_getdirentries", "sceKernelGetdirentries"),
    ("SYS_freebsd11_getdents", "sceKernelGetdents"),
    // The process exit is `SYS__exit` in FreeBSD's table (entry 1 is the raw `_exit`), so stripping
    // `SYS_` already yields the name `orbistoun-libc` answers to. The thread exit, `SYS_thr_exit`
    // (431), has no implementation and is not bound to the process exit.
];

/// Numbers this must not bind even though the name matches.
///
/// `SYS_syscall` and `SYS___syscall` are the indirect forms: the number they carry is another
/// number, in the first argument. Binding them to anything called `syscall` would perform the wrong
/// call with the arguments shifted by one.
const NOT_A_CALL: &[&str] = &["SYS_syscall", "SYS___syscall"];

/// What each syscall number performs, for the numbers something here implements.
///
/// The number-to-name mapping is harvested from `sys/sys/syscall.h`, so the numbers stay traceable
/// to the header (D378). A number whose name nothing implements is absent, and the dispatcher
/// answers it as a kernel does.
pub(crate) fn syscalls() -> std::collections::BTreeMap<u64, (&'static str, orbistoun_core::GuestFn)>
{
    let implemented: std::collections::BTreeMap<&'static str, orbistoun_core::GuestFn> =
        implementations().into_iter().collect();
    let renamed: std::collections::BTreeMap<&str, &str> =
        SPELT_DIFFERENTLY.iter().copied().collect();

    let mut out = std::collections::BTreeMap::new();
    // The target's own numbers first, then FreeBSD's. Separate files because one is generated from
    // headers and the other records what guests asked for (D403).
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
/// `ENOSYS` negated, as a FreeBSD syscall reports failure to its calling stub. Harvested, with a
/// fallback that is still a detectable failure if the table cannot be read.
pub(crate) fn syscall_refusal() -> u64 {
    let enosys = orbistoun_hle::constants::abi_constant("errno", "ENOSYS").unwrap_or(1);
    (-enosys) as u64
}

/// Every declared symbol, sorted.
pub(crate) fn all(hasher: &NidHasher) -> Vec<DeclaredSymbol> {
    // Both tables: a function answering in `xmm0` is as implemented as one answering in `rax`
    // (D268).
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
                nid: if let Some(hex) = i.name.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16).unwrap_or_else(|_| hasher.hash(i.name).as_raw())
                } else {
                    hasher.hash(i.name).as_raw()
                },
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

    /// The list the stub table and the call trace both walk is stable across calls (D366).
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

    /// Everything implemented is reachable by name, not only by import. Payloads resolve most of
    /// their C library at run time, so a function that cannot be found by name cannot be called
    /// (D365).
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

    /// The resolver a payload asks for first is one of the names it can resolve (D365).
    #[test]
    fn the_resolver_can_resolve_itself() {
        let reachable: std::collections::BTreeSet<&str> =
            super::resolvable().into_iter().map(|(n, _)| n).collect();
        assert!(reachable.contains("sceKernelDlsym"));
    }

    /// The syscall mapping is harvested and binds the well-known numbers to the right names (D378).
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
            // Entry 1 is the raw `_exit`, not the `exit(3)` wrapper.
            ("_exit", 1),
        ] {
            let bound = table
                .get(&number)
                .unwrap_or_else(|| panic!("{name} is {number}"));
            assert_eq!(bound.0, name, "{number} must perform {name}");
        }
    }

    /// The vendor kernel-log write (601) binds, through the vendor table and a rename.
    ///
    /// 601 comes from `vendor-syscalls.toml` and reaches its function through a `SPELT_DIFFERENTLY`
    /// rename. Both steps fail silently, leaving the number answering `ENOSYS`.
    #[test]
    fn the_vendor_klog_syscall_binds_to_the_log_write() {
        let table = super::syscalls();
        let bound = table
            .get(&601)
            .expect("syscall 601 must bind rather than answer ENOSYS");
        assert_eq!(
            bound.0, "sceKernelDebugOutText",
            "601 writes the operator log, the same function the named call does"
        );
    }

    /// Every rename names a constant that exists.
    ///
    /// A `SPELT_DIFFERENTLY` key that matches no harvested constant looks like a binding and does
    /// nothing, so the shape is asserted for every entry.
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

    /// No symbol is declared twice.
    #[test]
    fn no_symbol_is_declared_twice() {
        // A duplicate would mean two subsystems claim one function, and the registry's last-wins
        // rule would pick one silently.
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

    /// Everything implemented, integer and floating-point, is reachable through the registry a run
    /// builds.
    ///
    /// Tested here because a subsystem crate can only test a registry it builds itself, and no run
    /// builds that one: `modules()` is the single list.
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
            let nid = if let Some(hex) = name.strip_prefix("0x") {
                u64::from_str_radix(hex, 16)
                    .map_or_else(|_| hasher.hash(name), orbistoun_nid::Nid::from_raw)
            } else {
                hasher.hash(name)
            };
            assert!(
                registry.resolve(nid).is_some(),
                "{name} is implemented, but no module declaring it reaches the registry"
            );
        }
    }

    /// Every module contributes at least one symbol.
    #[test]
    fn every_module_contributes_at_least_one_symbol() {
        for m in modules() {
            assert!(!m.imports.is_empty(), "{} declares nothing", m.name);
        }
    }

    /// Distinct symbols hash to distinct NIDs.
    #[test]
    fn nids_differ_per_symbol() {
        // A collision would make two functions indistinguishable at resolution time.
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

    /// Libraries that declare functions and serve none, exactly, each with its reason.
    ///
    /// The exact set, so a library gaining an implementation fails until its entry is deleted, and
    /// one losing its registration fails until an entry is added and justified.
    // Each entry's reason opens with the count of names the library declares, which a test checks.
    // A module that serves nothing goes here with its reason.
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

    /// A declared library that serves nothing is either listed above or a bug: a module registered
    /// in `modules()` whose `implementations()` were never gathered declares functions nobody
    /// answers.
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
                // Named arguments, because `concat!` is not a literal and implicit capture needs
                // one.
                concat!(
                    "{} serves {serves} function(s) but is still listed as serving nothing - ",
                    "delete its entry from SERVES_NOTHING"
                ),
                module.name,
                serves = serves
            );
        }
    }

    /// An excuse that counts the names counts them right.
    ///
    /// Every reason opens `declared as N name(s)`, and N is checked against the module's
    /// declarations. A reason that does not open with that phrase is bespoke and has nothing to
    /// check.
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

        // The count of entries checked is asserted, so a loop that silently checked none fails.
        assert_eq!(
            checked,
            SERVES_NOTHING.len(),
            "every entry states a name count today, so all of them should have been checked"
        );
    }

    /// Every excuse names a library that is actually declared, so a renamed or deleted module does
    /// not leave a stale entry.
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

    /// The declared arity and the recorded arity never disagree.
    #[test]
    fn declared_arity_and_recorded_arity_never_disagree() {
        // The `guest_module!` declaration and the knowledge file may each be incomplete, but must
        // never contradict each other on arity (D122).
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

    /// Every implemented function is written down in the knowledge file.
    #[test]
    fn every_implemented_function_is_written_down() {
        // Every implemented function has a knowledge entry, so what was learned is recorded.
        let knowledge = Knowledge::builtin();
        for (name, _) in super::implementations() {
            assert!(
                knowledge.get(name).is_some(),
                "{name} is implemented but nothing is recorded about it"
            );
        }
    }

    /// A recorded argument list matches the recorded arity.
    #[test]
    fn a_recorded_argument_list_matches_the_recorded_arity() {
        // A recorded argument list agrees with the recorded arity.
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
