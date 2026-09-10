//! `sceKernelDlsym` resolves a platform name on the payload route.
//!
//! # Why the two routes are two test binaries
//!
//! Both the route a run presents and the table of stubs a lookup may answer with are
//! `OnceLock`s - set once by the loader, first setter wins. A test that sets either decides
//! for every test sharing its process, so each route gets a binary of its own and installs
//! its own table. [`dlsym_route_title`](../dlsym_route_title.rs) is the other half.
//!
//! # What sent them here
//!
//! orbistoun resolved every platform name by name, on both routes. obSCEne's
//! `060-module/dlsym-resolves-known-symbol` **fails** `0x8002_0003` on the package leg of
//! sweep 20260909-234847 - a known symbol, a valid handle - and every `dlsym` measurement in
//! that leg reads `0x0`.
//!
//! It was not only a wrong answer. obSCEne's title bootstrap resolves `getpid` to derive
//! libkernel's base, and reads a non-zero base as *"this process is a payload"* - so orbistoun
//! running a native title reported itself as `payload/unknown-gpu`, and every conformance
//! comparison went against the wrong leg of the sweep (D669).

use orbistoun_core::route::{Route, present};
use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// A name that is implemented, so a refusal cannot be mistaken for "nothing implements it".
const IMPLEMENTED: &str = "sceKernelUsleep";

/// Where the pretend stub lives. Nothing is called, so any address will do - but not zero, and
/// not one that could be confused with a real answer.
const THUNK_AT: u64 = 0x0000_7000_0000_1234;

fn dlsym() -> GuestFn {
    orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == "sceKernelDlsym")
        .map_or_else(|| panic!("no guest can reach sceKernelDlsym"), |(_, f)| *f)
}

/// A NUL-terminated guest string, kept alive by the returned buffer.
fn guest_string(text: &str) -> (Vec<u8>, u64) {
    let mut storage = text.as_bytes().to_vec();
    storage.push(0);
    let at = storage.as_mut_ptr().expose_provenance() as u64;
    (storage, at)
}

/// **A payload resolves platform names, and has to.**
///
/// The open-toolchain runtime asks the platform for its C library a name at a time and reaches
/// `main` with a table of nulls otherwise - the wall three sessions of diagnostics arrived at
/// from different directions (D365). Refusing on both routes would fix a title and break every
/// payload, so this is the half that says the narrowing stopped where the measurement did.
#[test]
fn a_payload_resolves_a_platform_name() {
    present(Route::Payload);
    orbistoun_thunk::install_name_thunks(
        [(IMPLEMENTED.to_owned(), THUNK_AT)].into_iter().collect(),
    );

    let (_storage, name) = guest_string(IMPLEMENTED);
    let mut answered: u64 = 0;
    let out = std::ptr::from_mut(&mut answered).expose_provenance() as u64;

    let mut args = [0_u64; GUEST_ARG_REGISTERS];
    // libkernel's handle, which is the one obSCEne asks through.
    args[0] = 0x2001;
    args[1] = name;
    args[2] = out;

    assert_eq!(dlsym()(&args), 0, "a payload's resolver answers");
    assert_eq!(
        answered, THUNK_AT,
        "with the same stub an import would have been bound to"
    );
}
