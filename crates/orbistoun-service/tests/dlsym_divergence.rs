//! `sceKernelDlsym` answers an address where the console answers "no such symbol".
//!
//! # Why this is a file of its own
//!
//! It installs a name-thunk table, and that table is a process-wide `OnceLock` - the first
//! call wins for the whole test binary. Putting this beside the other hardware claims would
//! silently decide what every other test in that binary sees, so it gets its own.
//!
//! # The divergence
//!
//! obSCEne's `110-modules/symbol` loads libkernel - handle `0x2001`, a value three separate
//! measurements agree on - and asks it for `memcpy`. The console answers `0x80020003`, ESRCH:
//! **libkernel does not export `memcpy`.**
//!
//! Orbistoun publishes every function it implements by name, in one flat table
//! (`symbols::resolvable()`, installed as `NAME_THUNKS`), and `dlsym` looks a name up there
//! without reference to the module handle it was given. So it answers `0` and writes an
//! address - success, for a symbol the module named does not have.
//!
//! That is plausible output in the sense principle 3 means it: a guest asking *whether* a
//! symbol exists is told yes, and cannot tell that nothing consulted the module it asked
//! about. It is recorded rather than fixed because the fix needs something this project does
//! not have - a per-module export list for the platform's own libraries - and inventing one
//! would be a worse answer than a wrong one, it would be a fabricated one.
//!
//! # The trap this file exists to stop
//!
//! The measurement is `0x80020003`, and **orbistoun already answers `0x80020003` from
//! `dlsym`** - for a *negative* module handle, the branch obSCEne's
//! `060-module/dlsym-rejects-bad-handle` covers (D366). A test written to claim the
//! measurement with an invalid handle would pass, in green, having exercised a branch that
//! has nothing to do with what was measured. Check 11 of the loop notes, and check 19: ask
//! whether the branch was reached before reasoning about what it returned.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::hardware::Measurements;

/// An address that is obviously not a real one, so a test reading it back cannot mistake it
/// for something resolved by accident.
const PRETEND_ADDRESS: u64 = 0x1234_5678;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let found = orbistoun_service::implementation_named(name)
        .unwrap_or_else(|| panic!("{name} is not implemented, so nothing can be checked"));
    found(&args)
}

/// **Recorded divergence: the module handle does not narrow what `dlsym` will answer.**
///
/// # What this asserts
///
/// Three things, and the third is the point:
///
/// 1. The console's measurement is ESRCH, read from the table rather than written here.
/// 2. Orbistoun answers ESRCH for a **negative** handle - the branch that already matches, and
///    the one a careless claim of this measurement would land in.
/// 3. Given libkernel's own handle and a name in the thunk table, orbistoun answers `0` and
///    writes the address. It disagrees with the console, and it disagrees by succeeding.
///
/// It is green on purpose. This file's doctrine is that a divergence lives in `OUTSTANDING`
/// with a reason rather than in a test that is red forever, because a permanently red build is
/// something people learn to ignore. What a test can still do is **notice when the divergence
/// stops being the one that was written down** - if somebody scopes resolution to the module,
/// this fails and sends them to the entry.
///
/// # What it cannot assert
///
/// That the runtime table contains `memcpy`. It contains what `symbols::resolvable()` builds,
/// which is every implementation, but that map is installed during a load this test does not
/// perform - so the table here is one entry, stood in for it. What is being checked is that
/// `dlsym` consults the table *at all* and ignores the handle while doing it, which is the
/// behaviour that diverges; the real table only makes the divergence wider.
///
/// And it says nothing about whether the console's ESRCH is about `memcpy` specifically or
/// about what that process had loaded. One capture, one application category - the same
/// caveat the encoder entries in `hardware.rs` carry.
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
        "the console answered ESRCH for memcpy out of libkernel; this test is about that \
         value and nothing else, got {console:#x}"
    );

    let name = std::ffi::CString::new("memcpy").expect("a name with no interior nul");
    let mut out: u64 = 0;
    let out_at = std::ptr::from_mut(&mut out) as u64;

    // (2) The branch that already agrees, and the reason a claim here would be wrong.
    let bad_handle = call(
        "sceKernelDlsym",
        [u64::from(u32::MAX), name.as_ptr() as u64, out_at, 0, 0, 0],
    );
    assert_eq!(
        bad_handle as u32, 0x8002_0003,
        "an invalid handle earns ESRCH (D366) - the same value the console answered for a \
         different reason entirely, which is why claiming the measurement through this path \
         would be green and meaningless"
    );
    // (3) The divergence, as it stands now. One entry stands in for the table a load installs.
    let mut named = std::collections::BTreeMap::new();
    named.insert("memcpy".to_owned(), PRETEND_ADDRESS);
    orbistoun_thunk::install_name_thunks(named);

    // **Before a libkernel list is published, nothing is refused.** "Nobody said which module
    // exports this" must not become "this module does not export it", so the narrowing below
    // is inert until somebody publishes an answer - and this is the state a run gets if the
    // service never does (D629).
    let unnarrowed = call(
        "sceKernelDlsym",
        [0x2001, name.as_ptr() as u64, out_at, 0, 0, 0],
    );
    assert_eq!(
        unnarrowed, 0,
        "with no list published, the flat table answers as it always did"
    );

    // **With the list published, the handle narrows the answer to the console's.** libkernel
    // does not export `memcpy`; this now says so.
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
        "libkernel's handle and a name libkernel does not declare now answers what the console \
         answered, which is the whole of this measurement"
    );
    assert_eq!(
        out, 0,
        "and writes nothing, as the console did - a refusal that filled the out-parameter would \
         leave a caller acting on an address it was told it did not get"
    );

    // **A name libkernel *does* declare still resolves through the same handle**, which is the
    // half that makes this a narrowing rather than a refusal. Written because a guard that
    // refused everything would also pass the assertion above.
    let keeper = std::ffi::CString::new("scePthreadCreate").expect("no interior NUL");
    let still = call(
        "sceKernelDlsym",
        [0x2001, keeper.as_ptr() as u64, out_at, 0, 0, 0],
    );
    assert_eq!(
        still as u32, 0x7fff_0001,
        "it is declared in libkernel, so the narrowing lets it through to the ordinary answer - \
         which here is 'nothing implements it', because this test installed no thunk for it"
    );
}
