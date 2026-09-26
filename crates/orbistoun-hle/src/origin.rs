//! Where an import's implementation has to come from, judged by the library it names.
//!
//! Call count says what a guest leans on, not what to write next. A documented function can be
//! written and tested from its standard in bulk, with no guest; a module the title ships is
//! loaded, never reimplemented, because guest code runs natively and only the system beneath it
//! is ours; only a vendor library needs the guest as its oracle and is worked one call at a
//! time (D472).

/// Where the implementation of an import has to come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// A published interface: the C library, POSIX, the C++ ABI.
    ///
    /// Writable from its specification and testable against it, so these are done in bulk and out
    /// of order. The standard is the oracle.
    Documented,
    /// A module the title ships in its own directory.
    ///
    /// Not implemented here: the code exists and the job is to load it.
    TitleOwn,
    /// A platform library with no published semantics.
    ///
    /// The guest is the only oracle, so these are found and answered one at a time.
    Vendor,
}

/// The libraries whose contents are a published standard rather than a vendor's invention.
///
/// `libc` carries the C library and, on this platform, the C++ runtime and its ABI;
/// `libScePosix` is POSIX under a vendor name. The `sce`-prefixed functions inside `libc` are
/// judged by their own name below.
const DOCUMENTED: &[&str] = &["libc", "libScePosix", "libkernel_fs"];

/// Where an import has to come from, given the library it names and the symbol it wants.
///
/// The symbol is consulted because `libc` also holds the platform's own allocator
/// (`sceLibcMspaceMalloc` and its family), which is vendor code.
#[must_use]
pub fn of(library: &str, symbol: &str) -> Origin {
    // A title-shipped module has no `lib` prefix: it is the title's own name for its own code.
    // Judged first, since a title may name a module anything.
    if !library.starts_with("lib") {
        return Origin::TitleOwn;
    }
    if DOCUMENTED.contains(&library) && !symbol.starts_with("sce") {
        return Origin::Documented;
    }
    Origin::Vendor
}

#[cfg(test)]
mod tests {
    use super::{Origin, of};

    /// The ordinary C library, which is the whole point of the distinction.
    #[test]
    fn the_c_library_is_documented() {
        assert_eq!(of("libc", "strlen"), Origin::Documented);
        assert_eq!(of("libc", "setenv"), Origin::Documented);
        assert_eq!(of("libScePosix", "pthread_create"), Origin::Documented);
    }

    /// The C++ runtime inside `libc` is published: the Itanium ABI and the C11 threads both have
    /// specifications to write against.
    #[test]
    fn the_cxx_runtime_inside_libc_is_documented() {
        assert_eq!(of("libc", "_Unwind_Resume"), Origin::Documented);
        assert_eq!(of("libc", "_Thrd_join"), Origin::Documented);
        assert_eq!(of("libc", "_ZNKSt9exception6_RaiseEv"), Origin::Documented);
    }

    /// The platform's own allocator lives in `libc` and is still vendor code.
    #[test]
    fn a_vendor_function_inside_libc_is_still_vendor() {
        assert_eq!(of("libc", "sceLibcMspaceMalloc"), Origin::Vendor);
        assert_eq!(of("libc", "sceLibcMspaceFree"), Origin::Vendor);
    }

    /// A module the title ships is the game's own code, whatever it is called.
    #[test]
    fn a_module_the_title_ships_is_its_own() {
        assert_eq!(of("Il2CppUserAssemblies", "il2cpp_init"), Origin::TitleOwn);
        assert_eq!(of("PS5Util", "anything"), Origin::TitleOwn);
    }

    /// A platform library is vendor.
    #[test]
    fn a_platform_library_is_vendor() {
        assert_eq!(of("libkernel", "sceKernelDlsym"), Origin::Vendor);
        assert_eq!(of("libSceAgc", "sceAgcDrawIndex"), Origin::Vendor);
    }
}
