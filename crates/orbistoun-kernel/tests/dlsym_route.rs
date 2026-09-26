//! `sceKernelDlsym` resolves a platform name on the payload route (D669).
//!
//! The route and the stub table a lookup answers with are both `OnceLock`s, set once per
//! process, so each route has its own test binary; [`dlsym_route_title`](../dlsym_route_title.rs)
//! is the other half. On the hardware a title's `dlsym` for a platform name fails, and a guest
//! that sees one resolve takes itself for a payload.

use orbistoun_core::route::{Route, present};
use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// A name that is implemented, so a refusal cannot be mistaken for "nothing implements it".
const IMPLEMENTED: &str = "sceKernelUsleep";

/// Where the pretend stub lives. Nothing is called, so any non-zero address that cannot be
/// mistaken for a real answer will do.
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

/// A payload resolves platform names.
///
/// The open-toolchain runtime asks the platform for its C library one name at a time and
/// reaches `main` with a table of nulls otherwise (D365).
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
