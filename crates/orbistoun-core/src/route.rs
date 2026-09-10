//! How a process reached the platform, and what that decides.
//!
//! # Two routes, and they are not the same sandbox
//!
//! A **payload** is mapped and jumped to by a homebrew loader. A **title** is installed and
//! launched by the platform's own loader. The executable can be byte-identical - SELFish's
//! pipeline wraps one payload four ways - so this is not a property of the file. It is a
//! property of how the process was started, and the platform behaves differently either side
//! of it.
//!
//! The difference measured so far is resolution by name. A title's `sceKernelDlsym` answers
//! `0x8002_0003` for a **known** symbol from a **valid** handle: obSCEne's
//! `060-module/dlsym-resolves-known-symbol` fails on the package leg of sweep
//! 20260909-234847, every `dlsym` measurement in that leg is `0x0`, and its
//! `005-generation/detect` says in as many words that this platform does not resolve modules
//! by name. A payload's does resolve, and has to: the open-toolchain runtime asks for its C
//! library a name at a time and reaches `main` with a table of nulls otherwise (D365).
//!
//! # Why this is not the application category
//!
//! [`crate::category`] is the other axis and answers a different question - what a title
//! *declared about itself* in `param.json`, which decides direct memory and the scanout. This
//! one is not declared anywhere: nothing in the file says which loader started it, and the
//! only thing that knows is orbistoun, because orbistoun is the loader.
//!
//! The two are independent. A big app delivered as a payload is an ordinary thing to build.

/// How a process reached the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    /// Installed and launched by the platform's own loader.
    ///
    /// The default, because it is what `run <title>` does and what every retail guest in the
    /// corpus is. A run that never said is running a title.
    #[default]
    Title,
    /// Mapped and jumped to by a homebrew loader.
    Payload,
}

/// What a title's resolver answers for a name, whether or not the name exists.
///
/// `0x8002_0003` - `ESRCH` in the kernel's own base. **Measured**: obSCEne asked for a known
/// symbol from a valid module handle and got this, and asked with a deliberately bad handle
/// and got the same. The platform does not distinguish, because it does not look.
pub const NAME_NOT_RESOLVED: u32 = 0x8002_0003;

/// The route this run presents, if one was declared.
static PRESENTED: std::sync::OnceLock<Route> = std::sync::OnceLock::new();

/// Records how this process was started, once, before it is entered.
///
/// The same shape as the category and the machine, and for the same reason: a guest call
/// arrives on a frame with no room to thread a context through.
pub fn present(route: Route) {
    let _ = PRESENTED.set(route);
}

/// How this run presents itself.
#[must_use]
pub fn presented() -> Route {
    PRESENTED.get().copied().unwrap_or_default()
}

/// Whether the platform will resolve a symbol by name on this route.
///
/// **A refusal here is a measurement, not a limitation.** Answering an address where the
/// console answers `ESRCH` gives a guest a working path the hardware does not have, and every
/// finding taken through it describes an emulator rather than a platform.
#[must_use]
pub const fn resolves_by_name(route: Route) -> bool {
    match route {
        Route::Payload => true,
        Route::Title => false,
    }
}

/// The metadata files that make a directory a title rather than a folder with a binary in it.
///
/// `param.json` is the current generation's; `param.sfo` is what a package carries, and a
/// package installs *as* a title - the compatibility sandbox is still a launched title, not a
/// payload. Either one settles it.
const TITLE_METADATA: &[&str] = &["sce_sys/param.json", "sce_sys/param.sfo"];

/// Which route a module at this path was delivered by.
///
/// # Why the metadata file is the discriminator
///
/// It is the platform's own. SELFish puts it plainly: *what makes a title native is
/// `param.json` and native registration, not the magic* - the executable is byte-identical
/// across all four of its output formats, so nothing inside the file can answer this. What
/// differs is the directory it was installed into.
///
/// It also matches the corpus exactly, which is worth stating because a discriminator that
/// happened to be right about six of seven would be worse than none: every retail title here
/// carries `sce_sys/param.json`, and the one payload carries an `eboot.bin` and nothing else.
///
/// # The predicate is an argument
///
/// So the decision is testable without a filesystem, which is the shape `orbistoun-mem` uses
/// for address-space rules and the reason they can be checked without mapping anything.
/// # A bare filename is not a special case
///
/// `Path::new("eboot.bin").parent()` is `Some("")`, not [`None`] - the empty path meaning the
/// working directory - so `sce_sys/param.json` is looked for *there*, which is right: somebody
/// who runs a module from inside a title directory is running that title. The first version of
/// this carried a `None` arm calling that a payload, and the test written for it failed, which
/// is the only reason the arm is not still here answering for a case it never sees.
pub fn route_for(module: &std::path::Path, exists: impl Fn(&std::path::Path) -> bool) -> Route {
    let directory = module.parent().unwrap_or_else(|| std::path::Path::new(""));
    if TITLE_METADATA
        .iter()
        .any(|name| exists(&directory.join(name)))
    {
        Route::Title
    } else {
        Route::Payload
    }
}
#[cfg(test)]
mod tests {
    use super::{NAME_NOT_RESOLVED, Route, resolves_by_name};

    /// **A title does not resolve by name and a payload does.**
    ///
    /// The whole of the axis, and both halves matter. Refusing everywhere would break the
    /// payloads, whose runtime bootstraps itself one name at a time (D365); resolving
    /// everywhere is what orbistoun did, and it made a title look like a payload to a probe
    /// that asks.
    #[test]
    fn each_route_resolves_the_way_the_console_does() {
        assert!(
            !resolves_by_name(Route::Title),
            "obSCEne 060-module/dlsym-resolves-known-symbol fails 0x80020003 on the package leg"
        );
        assert!(
            resolves_by_name(Route::Payload),
            "a payload runtime reaches main with a table of nulls otherwise"
        );
    }

    /// The refusal is the kernel's own encoding of the errno, not a placeholder.
    ///
    /// Asserted against the code the console answered rather than against a constant this
    /// crate also owns, because the point of the value is that a guest compares with it.
    #[test]
    fn the_refusal_is_the_code_the_console_answered() {
        assert_eq!(NAME_NOT_RESOLVED, 0x8002_0003);
        assert_eq!(
            crate::GuestError::vendor(crate::errno::NO_SUCH).as_raw(),
            NAME_NOT_RESOLVED,
            "ESRCH in the kernel's base, which is what obSCEne read back twice"
        );
    }

    /// **A run that never said is running a title.**
    ///
    /// Not an arbitrary default: `run <title>` is the command this project is built around,
    /// every retail guest in the corpus arrives that way, and the payload legs are the ones
    /// that go out of their way to say so.
    #[test]
    fn nothing_declared_is_a_title() {
        assert_eq!(Route::default(), Route::Title);
    }

    /// **The corpus, decided without touching a disk.**
    ///
    /// Every path here is one this project actually runs, and the discriminator has to get all
    /// eight right - six of seven would be worse than none, because a route that is sometimes
    /// wrong is a sandbox that is sometimes wrong.
    #[test]
    fn the_corpus_is_split_the_way_the_platform_splits_it() {
        use std::path::Path;

        // The metadata each corpus entry actually has beside it, as of 2026-09-10.
        let has_param = |p: &Path| {
            p.to_string_lossy().contains("sce_sys/param.json")
                && !p.to_string_lossy().contains("obscene-payload")
                && !p.to_string_lossy().contains("payloads-mirror")
        };

        for title in [
            "titles/PPSA02664-app0/eboot.bin",
            "titles/PPSA25872-app0/eboot.bin",
            "titles/PPSA99980/eboot.bin",
        ] {
            assert_eq!(
                super::route_for(Path::new(title), has_param),
                Route::Title,
                "{title} carries sce_sys/param.json"
            );
        }
        for payload in [
            "titles/obscene-payload/eboot.bin",
            "payloads/ps5-payloads-mirror/klogsrv.elf",
        ] {
            assert_eq!(
                super::route_for(Path::new(payload), has_param),
                Route::Payload,
                "{payload} has an executable beside nothing"
            );
        }
    }

    /// **A package counts as a title**, because a package installs as one.
    ///
    /// The compatibility sandbox is still a launched title rather than a payload, so
    /// `param.sfo` settles it exactly as `param.json` does. Written because the obvious
    /// implementation checks one filename and the corpus does not currently contain the other -
    /// which is how a case with no example in front of you gets left out.
    #[test]
    fn a_package_carrying_only_a_param_sfo_is_still_a_title() {
        use std::path::Path;
        let only_sfo = |p: &Path| p.ends_with("sce_sys/param.sfo");
        assert_eq!(
            super::route_for(Path::new("titles/UP0000-X/eboot.bin"), only_sfo),
            Route::Title
        );
    }

    /// **A bare filename is decided by the working directory**, which is where it resolves.
    ///
    /// Written expecting a payload, on the reading that a name with no directory has nothing
    /// beside it. `Path::parent` answers `Some("")` rather than `None`, so the lookup happens
    /// in the working directory - and somebody running a module from inside a title directory
    /// is running that title. This test failing is what removed a `None` arm that could never
    /// have fired.
    #[test]
    fn a_bare_filename_is_decided_by_what_is_in_the_working_directory() {
        use std::path::Path;
        let only_param = |p: &Path| p == Path::new("sce_sys/param.json");
        assert_eq!(
            super::route_for(Path::new("eboot.bin"), only_param),
            Route::Title
        );
        assert_eq!(
            super::route_for(Path::new("eboot.bin"), |_: &Path| false),
            Route::Payload
        );
    }
}
