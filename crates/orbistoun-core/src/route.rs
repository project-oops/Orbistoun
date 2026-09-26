//! How a process reached the platform, and what that decides.
//!
//! A payload is mapped and jumped to by a homebrew loader; a title is installed and launched by
//! the platform's own loader. The executable can be byte-identical, so the route is a property
//! of how the process started. A title's `sceKernelDlsym` does not resolve by name (obSCEne's
//! `060-module/dlsym-resolves-known-symbol` on the package leg); a payload's must, because the
//! open-toolchain runtime looks up its C library a name at a time (D365). This is independent
//! of [`crate::category`], which is what a title declares about itself; nothing in the file
//! declares its route, and only the loader knows it.

/// How a process reached the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    /// Installed and launched by the platform's own loader.
    ///
    /// The default: it is what `run <title>` does and how every retail guest arrives.
    #[default]
    Title,
    /// Mapped and jumped to by a homebrew loader.
    Payload,
}

/// What a title's resolver answers for a name, whether or not the name exists.
///
/// `0x8002_0003` - `ESRCH` in the kernel's own base. Measured for a known symbol from a valid
/// handle and for a bad handle alike: the platform does not look.
pub const NAME_NOT_RESOLVED: u32 = 0x8002_0003;

/// The route this run presents, if one was declared.
static PRESENTED: std::sync::OnceLock<Route> = std::sync::OnceLock::new();

/// Records how this process was started, once, before it is entered.
///
/// A static, like the category and the machine: a guest call arrives on a frame with no room to
/// thread a context through.
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
/// Answering an address where the hardware answers `ESRCH` would give a guest a path the
/// hardware does not have.
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
/// package installs as a title. Either one settles it.
const TITLE_METADATA: &[&str] = &["sce_sys/param.json", "sce_sys/param.sfo"];

/// Which route a module at this path was delivered by.
///
/// The discriminator is the title metadata beside the module, since the executable is the same
/// across delivery formats and only the install directory differs. The predicate is an argument
/// so the decision is testable without a filesystem. A bare filename's parent is `Some("")`,
/// the working directory, so a module run from inside a title directory is that title.
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

    /// A title does not resolve by name and a payload does.
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

    /// The refusal is the kernel's own encoding of the errno, asserted against the literal value
    /// a guest compares with.
    #[test]
    fn the_refusal_is_the_code_the_console_answered() {
        assert_eq!(NAME_NOT_RESOLVED, 0x8002_0003);
        assert_eq!(
            crate::GuestError::vendor(crate::errno::NO_SUCH).as_raw(),
            NAME_NOT_RESOLVED,
            "ESRCH in the kernel's base, which is what obSCEne read back twice"
        );
    }

    /// A run that never declared its route is running a title.
    #[test]
    fn nothing_declared_is_a_title() {
        assert_eq!(Route::default(), Route::Title);
    }

    /// Every corpus path is routed correctly, decided without touching a disk.
    #[test]
    fn the_corpus_is_split_the_way_the_platform_splits_it() {
        use std::path::Path;

        // The metadata each corpus entry has beside it.
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

    /// A package carrying only `param.sfo` is still a title.
    #[test]
    fn a_package_carrying_only_a_param_sfo_is_still_a_title() {
        use std::path::Path;
        let only_sfo = |p: &Path| p.ends_with("sce_sys/param.sfo");
        assert_eq!(
            super::route_for(Path::new("titles/UP0000-X/eboot.bin"), only_sfo),
            Route::Title
        );
    }

    /// A bare filename is decided by what is in the working directory.
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
