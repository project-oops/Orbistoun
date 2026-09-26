//! Naming the way a worker died.
//!
//! The exit status of a dead worker is a primary diagnostic, so it is rendered as the fault it
//! names rather than a bare number. The table is keyed on the exit code alone and has no
//! address, so it never says which cause a breakpoint had.
//!
//! Pure, and testable without killing anything (D016).

/// A fault this maps by name, with the platform code that identifies it.
///
/// Windows reports these as exit codes taken from the exception that killed the
/// process; Unix reports a signal number separately from the exit code.
#[cfg(windows)]
const FAULTS: &[(u32, &str)] = &[
    (
        0xC000_0005,
        "access violation - the guest dereferenced unmapped memory",
    ),
    (
        0xC000_001D,
        "illegal instruction - the guest jumped somewhere that is not code",
    ),
    (
        0xC000_00FD,
        "stack overflow - the guest ran past its guard page",
    ),
    (
        0xC000_0096,
        "privileged instruction - the guest attempted a kernel-only operation",
    ),
    (0xC000_0094, "integer divide by zero"),
    (0xC000_008C, "array bounds exceeded"),
    (
        0x8000_0003,
        // No address reaches this table, so it names no cause: the fault record tells stub
        // padding from a trap the guest executed.
        "breakpoint - a trap instruction; the fault record says whether it was stub padding",
    ),
];

/// Signals that mean the guest faulted, rather than that something asked it to stop.
#[cfg(unix)]
const FAULTS: &[(i32, &str)] = &[
    (
        4,
        "illegal instruction - the guest jumped somewhere that is not code",
    ),
    (
        5,
        "trap - stub padding, a trap the guest executed, or a debugger",
    ),
    (6, "abort"),
    (7, "bus error - a misaligned or unbacked access"),
    (8, "arithmetic fault"),
    (
        11,
        "segmentation fault - the guest dereferenced unmapped memory",
    ),
];

/// Describes a worker exit code in words.
///
/// On Windows an abnormal termination surfaces as the exception code itself, which is
/// why this reads as a fault table rather than an exit-code table.
#[cfg(windows)]
pub fn describe(code: Option<i32>, _signal: Option<i32>) -> String {
    let Some(code) = code else {
        return "the worker ended without reporting a status".to_owned();
    };
    if code == 0 {
        return "the worker exited cleanly without a verdict".to_owned();
    }
    if code == crate::report::TIME_LIMIT_EXIT {
        return "the guest was still running when its time limit expired".to_owned();
    }
    if code == crate::report::CALL_BUDGET_EXIT {
        return "the guest reached the call budget it was given".to_owned();
    }
    let raw = code as u32;
    FAULTS.iter().find(|(c, _)| *c == raw).map_or_else(
        || format!("the worker exited with status {raw:#010x}"),
        |(_, what)| format!("{what} ({raw:#010x})"),
    )
}

/// Describes a worker exit in words.
///
/// A signal, when present, always explains more than the exit code does.
#[cfg(unix)]
pub fn describe(code: Option<i32>, signal: Option<i32>) -> String {
    if let Some(signal) = signal {
        return FAULTS.iter().find(|(s, _)| *s == signal).map_or_else(
            || format!("the worker was killed by signal {signal}"),
            |(_, what)| format!("{what} (signal {signal})"),
        );
    }
    match code {
        Some(0) => "the worker exited cleanly without a verdict".to_owned(),
        Some(c) if c == crate::report::TIME_LIMIT_EXIT => {
            "the guest was still running when its time limit expired".to_owned()
        }
        Some(c) if c == crate::report::CALL_BUDGET_EXIT => {
            "the guest reached the call budget it was given".to_owned()
        }
        Some(code) => format!("the worker exited with status {code}"),
        None => "the worker ended without reporting a status".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::describe;

    /// A memory fault is named rather than left as a number.
    #[test]
    fn a_memory_fault_is_named_rather_than_left_as_a_number() {
        #[cfg(windows)]
        let described = describe(Some(0xC000_0005_u32 as i32), None);
        #[cfg(unix)]
        let described = describe(None, Some(11));

        assert!(
            described.contains("unmapped memory"),
            "expected a memory fault, got: {described}"
        );
    }

    /// A breakpoint reads as a trap instruction, not a bad pointer, without naming which kind of
    /// trap it was.
    #[test]
    fn a_breakpoint_is_distinguished_from_a_memory_fault() {
        #[cfg(windows)]
        let described = describe(Some(0x8000_0003_u32 as i32), None);
        #[cfg(unix)]
        let described = describe(None, Some(5));

        assert!(
            described.contains("padding"),
            "expected the padding case, got: {described}"
        );
    }

    /// An unrecognised status is reported as its number: hex on Windows, where exception codes
    /// are written that way, decimal on Unix.
    #[test]
    fn an_unknown_status_still_reports_the_number() {
        let described = describe(Some(42), None);
        #[cfg(windows)]
        let expected = "2a";
        #[cfg(unix)]
        let expected = "42";
        assert!(described.contains(expected), "got: {described}");
    }

    /// A clean exit without a verdict is reported as a worker defect, distinct from a fault.
    #[test]
    fn a_clean_exit_without_a_verdict_is_reported_as_such() {
        let described = describe(Some(0), None);
        assert!(described.contains("cleanly"), "got: {described}");
    }
}
