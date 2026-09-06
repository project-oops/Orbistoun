//! Every measured refusal code, checked against what orbistoun answers.
//!
//! # The gap this fills
//!
//! Two pipelines read the same conformance captures and they cover different things on
//! purpose. `OBS|measure|` records carry their own condition - check, subject, condition,
//! kind - so they can be *asserted*, and they become `hardware.toml`, which
//! `tests/hardware.rs` gates: every constant one is claimed by a test or declared outstanding.
//!
//! The pass/fail checks are the other pipeline. Their value's meaning lives in the check's own
//! C source rather than in the record - `sceKernelWrite` answering `0xffffffff80020009` to a
//! bad descriptor is a fact about the function, and `sceKernelGetProcessTime` answering `0xc3`
//! is the time it happened to be, and both arrive in the same field. So they are quoted into
//! knowledge entries as edge cases and deliberately never enter the assertable table.
//!
//! That is right, and it leaves **forty-seven measured values that no test looks at**. For
//! most of them the caution is the whole point. For the subset whose check id names a
//! *refusal* - `close-rejects-bad-handle`, `open-rejects-null`, `mutex-unlock-unheld` - the
//! meaning is not ambiguous, and the value is a code orbistoun either answers or does not
//! (D544).
//!
//! # Where the expected values come from
//!
//! The knowledge base, parsed at run time. Not a list copied into this file: a copied constant
//! drifts from the capture it came from and then passes for the wrong reason, which is the
//! failure D538 and D540 both landed on. If a re-absorbed capture changes one of these codes,
//! this file asks about the new one without anybody editing it.

use orbistoun_core::GUEST_ARG_REGISTERS;

/// The value obSCEne measured for `check`, as the entry for `function` records it.
///
/// Parsed out of the sentence `orbistoun-gen` writes, which is one fixed shape.
fn measured(function: &str, check: &str) -> u32 {
    let entry = orbistoun_hle::knowledge::Knowledge::builtin()
        .functions()
        .find(|f| f.name == function)
        .unwrap_or_else(|| panic!("{function} has no knowledge entry"))
        .clone();
    let head = format!("Measured on hardware: obSCEne `{check}` reported pass, value ");
    let sentence = entry
        .edge_cases
        .iter()
        .find(|e| e.starts_with(&head))
        .unwrap_or_else(|| panic!("{function} records no measurement from {check}"));
    let rest = &sentence[head.len()..];
    let digits: String = rest
        .trim_start_matches("0x")
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    let wide = u64::from_str_radix(&digits, 16)
        .unwrap_or_else(|e| panic!("{function}/{check} value {digits:?} is not hex: {e}"));
    // Some are recorded sign-extended (`0xffffffff80020009`) and some are not. A vendor code
    // is 32 bits; comparing the low half is comparing the code, and comparing the whole thing
    // would make two spellings of one value disagree.
    wide as u32
}

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let found = orbistoun_service::implementation_named(name)
        .unwrap_or_else(|| panic!("{name} is not implemented, so the refusal cannot be checked"));
    found(&args)
}

/// A handle no subsystem here ever issued.
const NEVER_ISSUED: u64 = 0xdead;

/// A descriptor far above anything the file table hands out.
const BAD_DESCRIPTOR: u64 = 0x1000;

/// Asserts that `function` answers the value obSCEne measured for `check`.
///
/// Returns nothing and panics on disagreement, so a caller counts its own cases - a helper
/// that returned a bool would let a test tally successes, which is the thing principle 3 says
/// not to assert on.
fn refuses(function: &str, check: &str, args: [u64; GUEST_ARG_REGISTERS], why: &str) {
    let expected = measured(function, check);
    let answered = call(function, args) as u32;
    assert_eq!(
        answered, expected,
        "{function} {why}: the console answered {expected:#010x}, orbistoun {answered:#010x}"
    );
}

/// **Every refusal that needs only a bad argument answers the console's code.**
///
/// # What this asserts
///
/// Eight conditions whose check id names what it did - rejecting a handle nothing issued, a
/// descriptor nothing opened, a null address. For each, that orbistoun's answer equals the
/// value obSCEne measured, read from the knowledge base rather than written here.
///
/// It is not a formality. These codes are per-subsystem and unrelated - `0x80260003` audio,
/// `0x80920003` input, `0x8029000b` video, `0x8002xxxx` kernel - so the agreements are separate
/// facts, and each is a code a guest can switch on.
///
/// # What it cannot assert
///
/// **That orbistoun was given the same input the console was.** The check id names the
/// *condition* and this reproduces a condition of that name; the actual handle or descriptor
/// obSCEne used lives in its C source. A match means "orbistoun answers the measured code when
/// refusing for the stated reason", not "on the same argument".
///
/// And nothing about conditions nobody measured: a subsystem answering `0x8029000b` for every
/// failure would pass here.
#[test]
fn refusals_that_need_only_a_bad_argument_answer_the_measured_code() {
    refuses(
        "sceAudioOutClose",
        "090-audio/close-rejects-bad-handle",
        [NEVER_ISSUED, 0, 0, 0, 0, 0],
        "on a handle it never issued",
    );
    refuses(
        "scePadClose",
        "100-input/close-rejects-bad-handle",
        [NEVER_ISSUED, 0, 0, 0, 0, 0],
        "on a handle it never issued",
    );
    refuses(
        "sceVideoOutClose",
        "080-video/close-rejects-bad-handle",
        [NEVER_ISSUED, 0, 0, 0, 0, 0],
        "on a handle it never issued",
    );
    refuses(
        "sceVideoOutSetFlipRate",
        "080-video/flip-rate-rejects-bad-handle",
        [NEVER_ISSUED, 1, 0, 0, 0, 0],
        "on a handle it never issued",
    );
    refuses(
        "sceKernelPollEventFlag",
        "015-sync/event-flag-rejects-bad-handle",
        [NEVER_ISSUED, 1, 0, 0, 0, 0],
        "on a handle it never issued",
    );
    refuses(
        "sceKernelClose",
        "040-file/close-rejects-bad-fd",
        [BAD_DESCRIPTOR, 0, 0, 0, 0, 0],
        "on a descriptor nothing opened",
    );
    refuses(
        "sceKernelLseek",
        "040-file/lseek-rejects-bad-fd",
        [BAD_DESCRIPTOR, 0, 0, 0, 0, 0],
        "on a descriptor nothing opened",
    );
    refuses(
        "sceKernelMunmap",
        "020-memory/unmap-rejects-null",
        [0, 0x1000, 0, 0, 0, 0],
        "on a null address",
    );
}

/// **Every refusal that needs a buffer, a path or an object answers the console's code.**
///
/// The other six, separated because they need something built first rather than because they
/// are a different kind of claim. Same caveat as above about condition versus argument, and one
/// case where that caveat has teeth.
///
/// # The path case, which is not one answer
///
/// `060-module/load-rejects-missing` loads a bogus path, and orbistoun's answer depends on
/// *which*: anything outside `libkernel`, the three firmware directories and `/app0/` is
/// refused with the measured ENOENT, but a nonexistent path **under `/app0/`** is handed a
/// fresh handle, because that is where a title's own modules live and the loader has already
/// placed them. That is deliberate, with the load recorded as having started nothing (D514).
///
/// Which branch obSCEne's check landed in is not knowable from inside this repository, so this
/// exercises the recognised-directory branch and says so rather than presenting one branch's
/// agreement as the function's.
#[test]
fn refusals_that_need_a_buffer_or_a_path_answer_the_measured_code() {
    let mut buffer = [0u8; 8];
    let at = buffer.as_mut_ptr() as u64;
    refuses(
        "sceKernelRead",
        "040-file/read-rejects-bad-fd",
        [BAD_DESCRIPTOR, at, 8, 0, 0, 0],
        "on a descriptor nothing opened",
    );
    refuses(
        "sceKernelWrite",
        "000-boot/write-rejects-bad-fd",
        [BAD_DESCRIPTOR, at, 8, 0, 0, 0],
        "on a descriptor nothing opened",
    );

    let nowhere = std::ffi::CString::new("/no-such-place/bogus.prx").expect("no interior nul");
    let nowhere_at = nowhere.as_ptr() as u64;
    refuses(
        "sceKernelOpen",
        "040-file/open-rejects-missing",
        [nowhere_at, 0, 0, 0, 0, 0],
        "on a path that is not there",
    );
    refuses(
        "sceKernelOpen",
        "040-file/open-rejects-null",
        [0, 0, 0, 0, 0, 0],
        "on a null path",
    );
    refuses(
        "sceKernelLoadStartModule",
        "060-module/load-rejects-missing",
        [nowhere_at, 0, 0, 0, 0, 0],
        "on a path in no directory it recognises",
    );

    // A mutex is initialised, then released by a caller that never took it. Performing that
    // would let two guest threads into one critical section, so the refusal is the behaviour
    // rather than an error path.
    let mut mutex: u64 = 0;
    let mutex_at = std::ptr::from_mut(&mut mutex) as u64;
    assert_eq!(
        call("scePthreadMutexInit", [mutex_at, 0, 0, 0, 0, 0]),
        0,
        "the mutex has to exist before unlocking it can be refused for the right reason"
    );
    refuses(
        "scePthreadMutexUnlock",
        "015-sync/mutex-unlock-unheld",
        [mutex_at, 0, 0, 0, 0, 0],
        "when the caller never held it",
    );
}
