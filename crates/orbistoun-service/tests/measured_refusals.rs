//! Every measured refusal code, checked against what orbistoun answers.
//!
//! `OBS|measure|` records carry their own condition and become `hardware.toml`, which
//! `tests/hardware.rs` gates. Pass/fail checks carry a value whose meaning lives in the check's C
//! source, so they enter knowledge entries as edge cases and never the assertable table. Where the
//! check id names a refusal (`close-rejects-bad-handle`, `open-rejects-null`,
//! `mutex-unlock-unheld`), the value is unambiguously a code, and this file asserts it (D545). The
//! expected values are parsed from the knowledge base at run time, so a re-absorbed capture is
//! checked without editing this file.

use orbistoun_core::GUEST_ARG_REGISTERS;

/// The value obSCEne measured for `check`, as the entry for `function` records it, parsed from the
/// one fixed sentence shape `orbistoun-gen` writes.
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
    // Some are recorded sign-extended (`0xffffffff80020009`) and some are not. A vendor code is 32
    // bits, so the low half is the code.
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

/// Asserts that `function` answers the value obSCEne measured for `check`. Panics on disagreement
/// rather than returning a bool a test could tally.
fn refuses(function: &str, check: &str, args: [u64; GUEST_ARG_REGISTERS], why: &str) {
    let expected = measured(function, check);
    let answered = call(function, args) as u32;
    assert_eq!(
        answered, expected,
        "{function} {why}: the console answered {expected:#010x}, orbistoun {answered:#010x}"
    );
}

/// Every refusal that needs only a bad argument answers the hardware's code.
///
/// Conditions whose check id names what was rejected: a handle nothing issued, a descriptor nothing
/// opened, a null address. The codes are per-subsystem and unrelated, so each agreement is a
/// separate fact a guest can switch on. A match means orbistoun answers the measured code when
/// refusing for the stated reason, not on the same argument; obSCEne's actual argument lives in its
/// C source.
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

/// Every refusal that needs a buffer, a path or an object answers the hardware's code.
///
/// `060-module/load-rejects-missing` depends on the path: anything outside `libkernel`, the three
/// platform module directories and `/app0/` is refused with the measured ENOENT, while a missing
/// path under `/app0/` gets a fresh handle that starts nothing (D515). Which branch obSCEne's check
/// reached is not knowable here, so this exercises the recognised-directory branch.
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

    // Unlocking a mutex the caller never took is refused; allowing it would let two guest threads
    // into one critical section.
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
