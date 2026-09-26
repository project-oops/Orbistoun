//! `sceKernelDlsym` narrows its answer by the module handle it is given.
//!
//! A file of its own because it installs a name-thunk table, a process-wide `OnceLock` whose first
//! install wins for the whole test binary.
//!
//! obSCEne's `110-modules/symbol` loads libkernel (handle `0x2001`) and asks it for `memcpy`; the
//! hardware answers `0x80020003`, ESRCH, because libkernel does not export `memcpy`. Orbistoun
//! answers ESRCH for a negative handle on a different branch, so a test claiming the measurement
//! with an invalid handle would pass without reaching the branch that matters; this one uses
//! libkernel's real handle.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::hardware::Measurements;

/// An address that is obviously not a real one, so a test reading it back cannot mistake it for
/// something resolved by accident.
const PRETEND_ADDRESS: u64 = 0x1234_5678;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let found = orbistoun_service::implementation_named(name)
        .unwrap_or_else(|| panic!("{name} is not implemented, so nothing can be checked"));
    found(&args)
}

/// The module handle narrows what `dlsym` answers.
///
/// Asserted: the hardware's measurement is ESRCH, read from the table; orbistoun answers ESRCH for
/// a negative handle; with no libkernel list published, a name in the thunk table resolves; with
/// the list published, libkernel's handle refuses `memcpy` as the hardware does and still resolves
/// a name libkernel declares. One entry stands in for the table a load installs, since this test
/// performs no load.
#[test]
fn dlsym_ignores_the_module_and_answers_where_the_console_refuses() {
    let measured = Measurements::builtin()
        .get("110-modules/symbol:sceKernelDlsym:memcpy")
        .expect("the measurement is in the table")
        .clone();
    let console = measured
        .value()
        .expect("a constant measurement has a value");
    assert_eq!(
        console as u32, 0x8002_0003,
        concat!(
            "the console answered ESRCH for memcpy out of libkernel; this test is about that ",
            "value and nothing else, got {:#x}"
        ),
        console
    );

    let name = std::ffi::CString::new("memcpy").expect("a name with no interior nul");
    let mut out: u64 = 0;
    let out_at = std::ptr::from_mut(&mut out) as u64;

    // The branch that already agrees for a negative handle.
    let bad_handle = call(
        "sceKernelDlsym",
        [u64::from(u32::MAX), name.as_ptr() as u64, out_at, 0, 0, 0],
    );
    assert_eq!(
        bad_handle as u32, 0x8002_0003,
        concat!(
            "an invalid handle earns ESRCH (D366) - the same value the console answered for a ",
            "different reason entirely, which is why claiming the measurement through this path ",
            "would be green and meaningless"
        )
    );
    // The payload route, declared first: on a title's route no platform symbol resolves by name at
    // all (D669), so the handle narrowing is a payload-route mechanism.
    orbistoun_core::route::present(orbistoun_core::route::Route::Payload);

    // One entry stands in for the table a load installs.
    let mut named = std::collections::BTreeMap::new();
    named.insert("memcpy".to_owned(), PRETEND_ADDRESS);
    orbistoun_thunk::install_name_thunks(named);

    // Before a libkernel list is published nothing is refused: "nobody said which module exports
    // this" must not become "this module does not export it".
    let unnarrowed = call(
        "sceKernelDlsym",
        [0x2001, name.as_ptr() as u64, out_at, 0, 0, 0],
    );
    assert_eq!(
        unnarrowed, 0,
        "with no list published, the flat table answers as it always did"
    );

    // With the list published, the handle narrows the answer to the hardware's.
    orbistoun_thunk::install_libkernel_names(
        ["sceKernelDlsym".to_owned(), "scePthreadCreate".to_owned()]
            .into_iter()
            .collect(),
    );
    out = 0;
    let narrowed = call(
        "sceKernelDlsym",
        [0x2001, name.as_ptr() as u64, out_at, 0, 0, 0],
    );
    assert_eq!(
        narrowed as u32, console as u32,
        concat!(
            "libkernel's handle and a name libkernel does not declare now answers what the console ",
            "answered, which is the whole of this measurement"
        )
    );
    assert_eq!(
        out, 0,
        concat!(
            "and writes nothing, as the console did - a refusal that filled the out-parameter would ",
            "leave a caller acting on an address it was told it did not get"
        )
    );

    // A name libkernel declares still resolves through the same handle, so this is a narrowing and
    // not a blanket refusal.
    let keeper = std::ffi::CString::new("scePthreadCreate").expect("no interior NUL");
    let still = call(
        "sceKernelDlsym",
        [0x2001, keeper.as_ptr() as u64, out_at, 0, 0, 0],
    );
    assert_eq!(
        still as u32,
        orbistoun_core::GuestError::Unimplemented.as_raw(),
        concat!(
            "it is declared in libkernel, so the narrowing lets it through to the ordinary answer - ",
            "which here is 'nothing implements it', because this test installed no thunk for it"
        )
    );
}
