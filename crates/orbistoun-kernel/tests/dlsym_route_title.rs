//! `sceKernelDlsym` refuses a platform name on a title's route.
//!
//! The other half of [`dlsym_route`](../dlsym_route.rs), in its own binary for the reason
//! stated there: the route and the stub table are both `OnceLock`s.
//!
//! **The title route is the default**, so nothing here sets it. The stub table *is* installed,
//! and that is what makes the assertion mean something: without it the name would resolve to
//! nothing anyway and the refusal would prove only that the table was empty.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// A name that is implemented and in the table below, so the only thing that can refuse it is
/// the route.
const IMPLEMENTED: &str = "sceKernelUsleep";

/// Where the pretend stub lives - the address a payload would have been given.
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

/// **A title does not resolve a platform name, which is what the console does.**
///
/// Measured: obSCEne asks libkernel - a valid handle - for a symbol it knows exists, and the
/// package leg of sweep 20260909-234847 answers `0x8002_0003`. Every `dlsym` measurement in
/// that leg is `0x0`, and `005-generation/detect` says in as many words that this platform
/// does not resolve modules by name.
#[test]
fn a_title_does_not_resolve_a_platform_name() {
    orbistoun_thunk::install_name_thunks(
        [(IMPLEMENTED.to_owned(), THUNK_AT)].into_iter().collect(),
    );
    assert_eq!(
        orbistoun_thunk::name_thunk(IMPLEMENTED),
        Some(THUNK_AT),
        "the table holds it, so a refusal below can only come from the route"
    );

    let (_storage, name) = guest_string(IMPLEMENTED);
    let mut answered: u64 = 0;
    let out = std::ptr::from_mut(&mut answered).expose_provenance() as u64;

    let mut args = [0_u64; GUEST_ARG_REGISTERS];
    args[0] = 0x2001;
    args[1] = name;
    args[2] = out;

    assert_eq!(
        dlsym()(&args),
        u64::from(orbistoun_core::route::NAME_NOT_RESOLVED),
        "the code the console answered for a known symbol from a valid handle"
    );
    assert_eq!(
        answered, 0,
        "and nothing is written through the out-parameter"
    );
}
