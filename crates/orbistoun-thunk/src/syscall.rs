//! The syscall boundary: a number is a name the guest did not spell.
//!
//! Titles are intercepted at the library boundary. Open-toolchain payloads also call a
//! raw syscall gadget directly, by number (D376), so orbistoun serves the kernel too: `SYS_write`
//! is four, `write` is implemented, and the mapping is a harvested table. An unknown number
//! fails as the kernel fails, with `ENOSYS`, never with success. Each distinct unknown number is
//! reported once, like the `sysctl` report.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::dispatch::GuestFn;

/// Registers a gadget stub saves, in the order it saves them.
///
/// `rax` (the number), the five syscall argument registers, then `rcx`. `syscall` destroys
/// `rcx`, so callers pass the fourth argument in `r10`; `rcx` is saved only to expose a caller
/// that used the function convention by mistake.
pub const SAVED: usize = 7;

/// The table this run dispatches with, by number.
static TABLE: OnceLock<BTreeMap<u64, (&'static str, GuestFn)>> = OnceLock::new();

/// What this answers for a number nothing here implements.
///
/// `ENOSYS` negated, as a FreeBSD syscall reports failure to its stub. `ENOSYS` is harvested in
/// a crate that depends on this one, so whoever builds the table supplies it (D378).
static REFUSAL: OnceLock<u64> = OnceLock::new();

/// Publishes the syscall table for this run, and what an unknown number answers.
///
/// A second call is ignored, as for every process-wide table here.
pub fn install_syscalls(table: BTreeMap<u64, (&'static str, GuestFn)>, refusal: u64) {
    let _ = TABLE.set(table);
    let _ = REFUSAL.set(refusal);
}

/// What an unknown number answers.
///
/// Falls back to a plain negative one when nothing published a refusal: still a failure, not
/// the platform's specific one.
fn no_such_call() -> u64 {
    REFUSAL.get().copied().unwrap_or(-1_i64 as u64)
}

/// How many syscalls this run can answer.
#[must_use]
pub fn known_syscalls() -> usize {
    TABLE.get().map_or(0, BTreeMap::len)
}

/// Every number this run was asked for, with the name it stands for where there is one.
///
/// Read by the reporting layer once the guest has stopped; the dispatch path only sets a bit.
#[must_use]
pub fn syscalls_asked_for() -> Vec<(u64, Option<&'static str>)> {
    use std::sync::atomic::Ordering::Relaxed;

    let mut out = Vec::new();
    for (word, slot) in SEEN.iter().enumerate() {
        let mut bits = slot.load(Relaxed);
        while bits != 0 {
            let bit = u64::from(bits.trailing_zeros());
            bits &= bits - 1;
            let number = (word as u64) * 64 + bit;
            out.push((number, syscall_name(number)));
        }
    }
    out
}

/// The name a number stands for, if this run knows one.
#[must_use]
pub fn syscall_name(number: u64) -> Option<&'static str> {
    TABLE.get()?.get(&number).map(|(name, _)| *name)
}

/// Numbers this run has already reported, as a bitmap.
///
/// Sixty-four words, so every number below 4096 has a bit. FreeBSD's highest is under six
/// hundred; the fallback is for numbers no kernel defines.
static SEEN: [std::sync::atomic::AtomicU64; 64] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 64];

/// Numbers a bitmap cannot hold, which is none of the real ones.
const HIGHEST_TRACKED: u64 = 64 * 64;

/// Whether a number has been reported already.
///
/// The dispatcher runs on the guest's stack, which this emulator neither owns nor sized, so it
/// must not allocate or lock (D381). A bitmap of atomics does neither.
fn first_time_seen(number: u64) -> bool {
    use std::sync::atomic::Ordering::Relaxed;

    if number >= HIGHEST_TRACKED {
        // No kernel defines one this high, so it is reported every time.
        return true;
    }
    let word = (number / 64) as usize;
    let bit = 1_u64 << (number % 64);
    let Some(slot) = SEEN.get(word) else {
        return true;
    };
    slot.fetch_or(bit, Relaxed) & bit == 0
}

/// How many syscalls this run records in order.
///
/// The first few: a payload calls the kernel directly while setting itself up, and by the time
/// it is serving it calls names.
const RECORDED_SYSCALLS: usize = 64;

/// The numbers, in the order they were asked for, plus one so zero means empty.
static ORDER: [std::sync::atomic::AtomicU64; RECORDED_SYSCALLS] =
    [const { std::sync::atomic::AtomicU64::new(0) }; RECORDED_SYSCALLS];

/// The first argument of each, which is often the only one that says anything.
static ORDER_ARG0: [std::sync::atomic::AtomicU64; RECORDED_SYSCALLS] =
    [const { std::sync::atomic::AtomicU64::new(0) }; RECORDED_SYSCALLS];

/// How many have been asked for, whether or not they fitted.
static ASKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Records one call in the order it happened.
///
/// [`first_time_seen`] says which numbers were asked for; this says when, which decides what a
/// payload was doing (D388). Allocation-free and lock-free, like everything on this path.
fn record_in_order(number: u64, arguments: &[u64]) {
    use std::sync::atomic::Ordering::{Relaxed, Release};

    let slot = ASKED.fetch_add(1, Relaxed);
    let Ok(slot) = usize::try_from(slot) else {
        return;
    };
    let (Some(held), Some(held_arg)) = (ORDER.get(slot), ORDER_ARG0.get(slot)) else {
        return;
    };
    held_arg.store(arguments.first().copied().unwrap_or(0), Relaxed);
    // The number last, so a reader never sees a live number against a stale argument.
    held.store(number.wrapping_add(1), Release);
}

/// One syscall a guest made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AskedFor {
    /// The number.
    pub number: u64,
    /// Its first argument, which is often the only one that says anything.
    pub argument: u64,
    /// The name it stands for, where this run knows one.
    pub name: Option<&'static str>,
}

/// The syscalls a run made, in order, and how many there were.
#[derive(Debug, Clone, Default)]
pub struct Sequence {
    /// The ones this run kept, in the order they happened.
    pub made: Vec<AskedFor>,
    /// How many were made in total, which may be more than [`Self::made`] holds, so a truncated
    /// list never reads as complete.
    pub total: u64,
}

/// Every syscall this run made, in order, with its first argument.
///
/// Read once the guest has stopped.
#[must_use]
pub fn syscalls_in_order() -> Sequence {
    use std::sync::atomic::Ordering::{Acquire, Relaxed};

    let mut made = Vec::new();
    for (held, held_arg) in ORDER.iter().zip(ORDER_ARG0.iter()) {
        let number = held.load(Acquire);
        if number == 0 {
            break;
        }
        let number = number - 1;
        made.push(AskedFor {
            number,
            argument: held_arg.load(Relaxed),
            name: syscall_name(number),
        });
    }
    Sequence {
        made,
        total: ASKED.load(Relaxed),
    }
}

/// Performs one syscall, from the registers a gadget stub saved.
///
/// # Safety
///
/// `saved` must point at [`SAVED`] words a gadget stub wrote, which is the only way this is
/// reached.
pub unsafe extern "sysv64" fn orbistoun_syscall_dispatch(saved: *const u64) -> u64 {
    // SAFETY: the caller's contract - a gadget stub's own save buffer, `SAVED` words long.
    let saved = unsafe { std::slice::from_raw_parts(saved, SAVED) };
    let number = saved[0];

    // Recorded, not printed: printing formats and allocates, and this frame is on the guest's
    // stack. The reporting layer reads the record afterwards.
    let _ = first_time_seen(number);

    let Some((_, implementation)) = TABLE.get().and_then(|table| table.get(&number)) else {
        // Recorded before the refusal: an unimplemented number's position in the sequence matters most.
        record_in_order(number, &[saved[1]]);
        return no_such_call();
    };

    // The syscall convention's argument registers, in order; the fourth is `r10`.
    let arguments = [saved[1], saved[2], saved[3], saved[4], saved[5], saved[6]];
    record_in_order(number, &arguments);
    implementation(&arguments)
}

/// How many system calls the guest has made, readable cheaply while it runs.
///
/// A sampler reads this several times a second, so it is one relaxed load;
/// [`syscalls_in_order`] builds a `Vec` and is for after the guest stops.
#[must_use]
pub fn syscalls_made() -> u64 {
    ASKED.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    /// A number nothing implements fails the way a kernel fails, not with success.
    #[test]
    fn an_unknown_number_answers_the_kernels_refusal() {
        let saved = [0xFFFF_u64; super::SAVED];
        // SAFETY: `saved` is exactly `SAVED` words and lives for the call.
        let answered = unsafe { super::orbistoun_syscall_dispatch(saved.as_ptr()) };
        assert_ne!(answered, 0, "a refusal, not a success");
        assert!(
            (answered as i64) < 0,
            "and negative, which is how a syscall reports one"
        );
    }

    /// A number is reported once, however often it is asked for.
    #[test]
    fn a_number_is_reported_the_first_time_and_not_after() {
        assert!(super::first_time_seen(321), "the first ask is news");
        assert!(!super::first_time_seen(321), "the second is not");
        assert!(
            super::first_time_seen(322),
            "and a different number is its own news"
        );
    }

    /// Every number a kernel defines has a bit, so recording never allocates.
    #[test]
    fn every_number_a_kernel_defines_has_a_bit() {
        // FreeBSD's highest number is under six hundred; the bitmap covers 4096.
        assert!(super::first_time_seen(4095), "the last number with a bit");
        assert!(!super::first_time_seen(4095), "which is remembered");

        // One past the end is answered without touching the bitmap, every time.
        assert!(super::first_time_seen(super::HIGHEST_TRACKED));
        assert!(super::first_time_seen(super::HIGHEST_TRACKED));
    }
}
