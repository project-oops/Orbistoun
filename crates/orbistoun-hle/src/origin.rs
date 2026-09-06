//! Where an import's implementation has to come from, judged by the library it names.
//!
//! # Why this is a distinction worth making in code
//!
//! Two of the three answers are not "write it", and both were learned the expensive way.
//!
//! A guest's imports arrive as one undifferentiated list, and the loop that works through them
//! has been ranking it by **how often each was called** - which answers "what is this guest
//! leaning on" and not "what should somebody write next". Those come apart badly:
//!
//! - A function the platform documents - `strlen`, `setenv`, the C11 threads, the Itanium C++
//!   ABI - can be written from the standard, tested against the standard, and needs no guest at
//!   all. Waiting for a run to trip over it buys nothing and costs a session per function. The
//!   whole of [`Origin::Documented`] can be worked in bulk, ahead of any title needing it.
//! - A function in a module **the title ships itself** must not be written here at all. PPSA02664
//!   imports `il2cpp_init` and friends from `Il2CppUserAssemblies`, which is sitting in its own
//!   directory: implementing it would be reimplementing the game, when the premise of this
//!   emulator is that guest code runs natively and only the system beneath it is ours (D472).
//! - Only [`Origin::Vendor`] has the oracle problem that makes incremental work necessary: no
//!   published semantics, so the guest's own behaviour is the only thing that says what a
//!   function should answer.
//!
//! Ranking by call count hid all of that, because it is a fact about one run rather than about
//! where an answer can come from.

/// Where the implementation of an import has to come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// A published interface: the C library, POSIX, the C++ ABI.
    ///
    /// Writable from its specification and testable against it, so these can be done in bulk
    /// and out of order. **The standard is the oracle**, which is exactly what the vendor
    /// functions lack.
    Documented,
    /// A module the title ships in its own directory.
    ///
    /// **Not ours to implement.** The code exists; the job is to load it. An implementation
    /// written here would be a second, worse copy of something the guest already has.
    TitleOwn,
    /// A platform library with no published semantics.
    ///
    /// The guest is the only oracle, so these are found and answered one at a time - the
    /// incremental loop, and the only place it is justified.
    Vendor,
}

/// The libraries whose contents are a published standard rather than a vendor's invention.
///
/// `libc` carries the C library and, on this platform, the C++ runtime and its ABI with it;
/// `libScePosix` is POSIX under a vendor name, which is a naming fact rather than a semantic
/// one. The `sce`-prefixed *functions* inside `libc` are the exception and are judged by their
/// own name below.
const DOCUMENTED: &[&str] = &["libc", "libScePosix", "libkernel_fs"];

/// Where an import has to come from, given the library it names and the symbol it wants.
///
/// The symbol is consulted because `libc` is not uniformly documented: the platform puts its own
/// allocator (`sceLibcMspaceMalloc` and its family) in there beside the standard C functions, and
/// a vendor function does not become documented by the company it keeps.
#[must_use]
pub fn of(library: &str, symbol: &str) -> Origin {
    // A module the title ships has no `lib` prefix and no vendor prefix - it is the game's own
    // name for its own code. Judged first: a title is free to call a module anything, including
    // something that would otherwise look like a platform library.
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

    /// The C++ runtime the platform ships inside `libc` is published too - the Itanium ABI and
    /// the Dinkumware C11 threads both have specifications to write against.
    #[test]
    fn the_cxx_runtime_inside_libc_is_documented() {
        assert_eq!(of("libc", "_Unwind_Resume"), Origin::Documented);
        assert_eq!(of("libc", "_Thrd_join"), Origin::Documented);
        assert_eq!(of("libc", "_ZNKSt9exception6_RaiseEv"), Origin::Documented);
    }

    /// **The exception that makes the symbol worth consulting.** The platform's own allocator
    /// lives in `libc`, and being in a documented library does not document it.
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

    /// And the platform's own libraries, which are the only ones that need a guest to answer.
    #[test]
    fn a_platform_library_is_vendor() {
        assert_eq!(of("libkernel", "sceKernelDlsym"), Origin::Vendor);
        assert_eq!(of("libSceAgc", "sceAgcDrawIndex"), Origin::Vendor);
    }
}
