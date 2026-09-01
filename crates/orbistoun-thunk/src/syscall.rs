//! The syscall boundary: a number is a name the guest did not spell.
//!
//! # Why this exists at all
//!
//! Everything else here intercepts a guest at the *library* boundary: the guest calls a name,
//! a relocation put a stub at that name, and the stub says who was called. That is the whole
//! of D005 and it covers every commercial title measured.
//!
//! The open-toolchain payloads go under it. They keep a pointer to a raw syscall gadget and
//! call it directly - `klog_printf` renders its message with `vsnprintf` and then asks the
//! kernel to deliver it, by number, past every name (D376).
//!
//! So orbistoun has to be the kernel here as well as the library. That is a smaller job than
//! it sounds, because **a syscall number is a name the guest did not spell**: `SYS_write` is
//! four, `write` is already implemented, and the mapping between them is a table this
//! repository harvests rather than writes.
//!
//! # What a number this does not know must do
//!
//! Fail the way the kernel fails, not the way a stub does. A kernel answers an unknown call
//! with `ENOSYS`; a stub that answered success would tell a guest its request was performed.
//! The distinction matters more here than at the library boundary, because a program that
//! reaches this level is one that has already decided the ordinary route is not available.
//!
//! Every distinct unknown number is reported once, because it is a work item and the guest is
//! the only thing that knows which are wanted - the same shape as the `sysctl` report.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::dispatch::GuestFn;

/// Registers a gadget stub saves, in the order it saves them.
///
/// `rax` first because that is where the number is, then the five registers a syscall passes
/// arguments in, then `rcx`. **`rcx` is saved and not used**: a `syscall` instruction destroys
/// it, so the caller passes the fourth argument in `r10` instead - and keeping `rcx` beside it
/// is what would show a caller that used the ordinary function convention by mistake.
pub const SAVED: usize = 7;

/// The table this run dispatches with, by number.
static TABLE: OnceLock<BTreeMap<u64, (&'static str, GuestFn)>> = OnceLock::new();

/// What this answers for a number nothing here implements.
///
/// **Passed in rather than written down.** It is `ENOSYS` negated - how a FreeBSD syscall
/// reports failure to the stub that called it - and `ENOSYS` is a harvested number that lives
/// in a crate this one cannot depend on, because that crate depends on this one. So whoever
/// builds the table supplies it, and the value stays sourced (D378).
static REFUSAL: OnceLock<u64> = OnceLock::new();

/// Publishes the syscall table for this run, and what an unknown number answers.
///
/// A second call is ignored, as with every other process-wide table here: two guests in one
/// process is not something this supports.
pub fn install_syscalls(table: BTreeMap<u64, (&'static str, GuestFn)>, refusal: u64) {
    let _ = TABLE.set(table);
    let _ = REFUSAL.set(refusal);
}

/// What an unknown number answers.
///
/// Falls back to a plain negative one when nothing published a refusal, which is still a
/// failure a caller detects - just not the specific one the platform would give.
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
/// Read by the reporting layer once the guest has stopped. The dispatch path only sets a bit.
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
/// hundred, so in practice every call a guest can make is covered and the fallback below is
/// for a number no kernel defines.
static SEEN: [std::sync::atomic::AtomicU64; 64] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 64];

/// Numbers a bitmap cannot hold, which is none of the real ones.
const HIGHEST_TRACKED: u64 = 64 * 64;

/// Whether a number has been reported already.
///
/// # Why this cannot allocate
///
/// **The dispatcher runs on the guest's stack.** A guest calls the gadget, the gadget calls
/// this, and every frame from there down is on whatever stack the guest was using - so an
/// allocation here is an allocation on a stack this emulator does not own and did not size.
///
/// The first version kept a `BTreeSet` behind a mutex, exactly like the `sysctl` and `dlsym`
/// reports do. Those are called from the ordinary import path, on a frame this project
/// arranged; this one is not, and it faulted inside `BTreeSet::insert` on the first syscall a
/// guest ever made here. The fault reported itself as `vsnprintf`, because that was the last
/// import called, and cost a long afternoon (D381).
///
/// A bitmap of atomics allocates nothing, locks nothing, and is exactly as good at the one
/// question being asked.
fn first_time_seen(number: u64) -> bool {
    use std::sync::atomic::Ordering::Relaxed;

    if number >= HIGHEST_TRACKED {
        // No kernel defines one this high, so a guest asking is worth hearing about every
        // time rather than once.
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
/// The first few, which is where the interesting ones are: a payload asks the kernel directly
/// while it is setting itself up, and by the time it is serving it is calling names.
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
/// # Why the set was not enough
///
/// [`first_time_seen`] answers *which* numbers a guest asked for. It cannot answer **when**,
/// and that is the question that decides what a payload was doing: `ftpsrv` asks for `getpid`,
/// `601` and `616`, and whether those come before or after `Unable to change AuthID` is the
/// difference between "the privilege path uses them" and "the give-up path does" (D388).
///
/// Allocation-free and lock-free, for the reason everything on this path is (D381).
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
    /// How many were made in total, which may be more than [`Self::made`] holds.
    ///
    /// **Said rather than truncated silently**, because a list that stops without saying so
    /// reads as a complete account (D385).
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
/// `saved` must point at [`SAVED`] words a gadget stub wrote, which is what the emitted stub
/// does and the only way this is reached.
pub unsafe extern "sysv64" fn orbistoun_syscall_dispatch(saved: *const u64) -> u64 {
    // SAFETY: the caller's contract - a gadget stub's own save buffer, `SAVED` words long.
    let saved = unsafe { std::slice::from_raw_parts(saved, SAVED) };
    let number = saved[0];

    // **Recorded, not printed.** Every distinct number is worth reporting - whether a guest
    // reaches this boundary at all is the first thing to know - but not from here: printing
    // formats and allocates, and this frame is on the *guest's* stack. The reporting layer
    // reads the record afterwards, exactly as it does for call counts (D381).
    let _ = first_time_seen(number);

    let Some((_, implementation)) = TABLE.get().and_then(|table| table.get(&number)) else {
        // Recorded before the refusal: a number nothing implements is exactly the one whose
        // position in the sequence is worth knowing.
        record_in_order(number, &[saved[1]]);
        return no_such_call();
    };

    // The syscall convention's argument registers, in order. The fourth is `r10`, which is
    // the whole difference between this and an ordinary call.
    let arguments = [saved[1], saved[2], saved[3], saved[4], saved[5], saved[6]];
    record_in_order(number, &arguments);
    implementation(&arguments)
}

#[cfg(test)]
mod tests {
    /// A number nothing implements fails the way a kernel fails.
    ///
    /// **Not the way a stub does.** A stub answering success would tell a guest its request
    /// was performed, and a program that has reached the syscall boundary is one that already
    /// decided the ordinary route was unavailable - so it will not check twice.
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

    /// **The one thing this must not do is allocate** (D381).
    ///
    /// The dispatcher runs on the guest's stack, so an allocation here is one on a stack this
    /// emulator does not own. A bitmap of atomics is the whole implementation, and this
    /// asserts the shape that guarantees it: every number a kernel defines has a bit.
    #[test]
    fn every_number_a_kernel_defines_has_a_bit() {
        // Every number a kernel defines has a bit: FreeBSD's highest is under six hundred,
        // and the bitmap covers four thousand with room to spare.
        assert!(super::first_time_seen(4095), "the last number with a bit");
        assert!(!super::first_time_seen(4095), "which is remembered");

        // One past the end is answered without touching the bitmap, every time - a number no
        // kernel defines is worth hearing about on each ask rather than once.
        assert!(super::first_time_seen(super::HIGHEST_TRACKED));
        assert!(super::first_time_seen(super::HIGHEST_TRACKED));
    }
}
