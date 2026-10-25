//! Dispatches that reach guest memory by device address (D964), as the command processor sees
//! them.
//!
//! A dispatch whose ranges the host cannot know before it runs (D753, D758) is not passed over
//! where the backend maps guest memory into the device: it is prepared again device-addressed, and
//! its module reaches guest memory where it lies. The backend says whether it can
//! ([`offer`]), and after such a dispatch hands back the guest ranges the dispatch may have
//! written ([`note_written`]): a device store passes the host's write-watch and page protection by,
//! so the command processor tells every cache of guest memory of them ([`take_written`]).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The table a device-addressed module looks guest addresses up in, and where its push block names
/// it: what a backend builds from the guest memory it maps.
pub use orbistoun_translate::device_address::{
    DEVICE_TABLE_PUSH_WORD, DEVICE_TABLE_PUSH_WORDS, Mapped, Table, TableError,
};

static OFFERED: AtomicBool = AtomicBool::new(false);

/// Says the backend maps guest memory into the device and runs device-addressed dispatches.
pub fn offer() {
    OFFERED.store(true, Ordering::Release);
}

/// Whether the backend runs device-addressed dispatches.
#[must_use]
pub fn offered() -> bool {
    OFFERED.load(Ordering::Acquire)
}

fn written() -> &'static Mutex<Vec<(u64, u64)>> {
    static WRITTEN: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
    &WRITTEN
}

/// Records guest ranges, `(first byte, bytes)`, a device-addressed dispatch may have written.
pub fn note_written(ranges: &[(u64, u64)]) {
    written()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend_from_slice(ranges);
}

/// The ranges [`note_written`] recorded since the last take, merged where they touch.
#[must_use]
pub fn take_written() -> Vec<(u64, u64)> {
    let mut ranges = std::mem::take(
        &mut *written()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    ranges.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
    for (base, len) in ranges {
        match merged.last_mut() {
            Some((first, held)) if *first + *held >= base => {
                *held = (*held).max(base + len - *first);
            }
            _ => merged.push((base, len)),
        }
    }
    merged
}

/// Whether a dispatch was refused because the host cannot know before it runs what it reaches: a
/// descriptor or offset it computes from what only its run decides (D753, D914), or a global
/// access whose lanes' addresses or execution mask only its run decides (D758). Every such refusal
/// says the host cannot know; a device-addressed dispatch needs to know none of it.
#[must_use]
pub fn host_cannot_know(why: &str) -> bool {
    why.contains("cannot know")
}

/// How many of a program's first bytes key it as one prepared device-addressed: enough that another
/// program loaded at the same address is told apart, few enough to read before every dispatch.
pub const PROGRAM_KEY_BYTES: usize = 256;

/// The programs whose host trace could not know what they reach and which were then prepared
/// device-addressed, each by its address and a hash of its bytes (D970).
fn device_programs() -> &'static Mutex<std::collections::BTreeSet<(u64, u64)>> {
    static PROGRAMS: Mutex<std::collections::BTreeSet<(u64, u64)>> =
        Mutex::new(std::collections::BTreeSet::new());
    &PROGRAMS
}

/// A program's bytes, hashed (FNV-1a): another program loaded at the same address is another key.
fn bytes_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Records that the program at `address`, holding `bytes`, was refused by the host trace for what
/// it cannot know and prepared device-addressed: its later dispatches skip the host trace, whose
/// runs over every group and wave end in the same refusal (D970).
pub fn remember_device_addressed(address: u64, bytes: &[u8]) {
    device_programs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert((address, bytes_hash(bytes)));
}

/// Whether the program at `address`, holding `bytes`, is prepared device-addressed from the start
/// ([`remember_device_addressed`]).
#[must_use]
pub fn goes_device_addressed(address: u64, bytes: &[u8]) -> bool {
    device_programs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(&(address, bytes_hash(bytes)))
}

/// Why a dispatch's preparation stopped before running its waves on the host to find a buffer:
/// where the backend runs device-addressed dispatches, one whose buffers only a host run of a
/// sample of its waves, or of its groups, would find is prepared device-addressed instead, which
/// finds none of them (D981). It says the host cannot know, so [`host_cannot_know`] answers it.
pub const HOST_RUN_PASSED: &str = "the host cannot know it without running the dispatch's waves on the host, which a device-addressed preparation does not need (D981)";

thread_local! {
    /// Whether this thread's preparation runs a dispatch's waves on the host where device
    /// addressing is offered ([`allowing_host_runs`]).
    static HOST_RUNS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a preparation finds a buffer by running a dispatch's waves on the host (D914, D950,
/// D951): only where the backend runs no device-addressed dispatch, or inside
/// [`allowing_host_runs`]. Anywhere else the preparation is refused with [`HOST_RUN_PASSED`].
#[must_use]
pub fn host_runs_allowed() -> bool {
    !offered() || HOST_RUNS.get()
}

/// Runs `prepare` with host runs allowed: for a program whose device-addressed preparation was
/// refused, which the host run then still serves (D981).
pub fn allowing_host_runs<T>(prepare: impl FnOnce() -> T) -> T {
    let was = HOST_RUNS.replace(true);
    let prepared = prepare();
    HOST_RUNS.set(was);
    prepared
}

/// The programs whose preparation passed over a host run and whose device-addressed preparation was
/// then refused, each by its address and a hash of its bytes: their later dispatches run the host
/// run from the start (D981).
fn host_run_programs() -> &'static Mutex<std::collections::BTreeSet<(u64, u64)>> {
    static PROGRAMS: Mutex<std::collections::BTreeSet<(u64, u64)>> =
        Mutex::new(std::collections::BTreeSet::new());
    &PROGRAMS
}

/// Records that the program at `address`, holding `bytes`, is prepared with host runs: device
/// addressing refused it.
pub fn remember_host_runs(address: u64, bytes: &[u8]) {
    host_run_programs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert((address, bytes_hash(bytes)));
}

/// Whether the program at `address`, holding `bytes`, is prepared with host runs from the start
/// ([`remember_host_runs`]).
#[must_use]
pub fn needs_host_runs(address: u64, bytes: &[u8]) -> bool {
    host_run_programs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(&(address, bytes_hash(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host run passed over is a refusal the host cannot know past, so a device-addressed
    /// preparation follows it; allowed, it is allowed only inside the call.
    #[test]
    fn a_host_run_passed_over_goes_device_addressed() {
        assert!(host_cannot_know(HOST_RUN_PASSED));
        let inside = allowing_host_runs(|| HOST_RUNS.get());
        assert!(inside && !HOST_RUNS.get());
        remember_host_runs(0x7000, b"program");
        assert!(needs_host_runs(0x7000, b"program"));
        assert!(!needs_host_runs(0x7000, b"another"));
    }

    /// Ranges noted come back merged where they touch, once.
    #[test]
    fn written_ranges_come_back_merged_once() {
        note_written(&[
            (0x2_0000, 0x1_0000),
            (0x1_0000, 0x1_0000),
            (0x5_0000, 0x1000),
        ]);
        assert_eq!(
            take_written(),
            vec![(0x1_0000, 0x2_0000), (0x5_0000, 0x1000)]
        );
        assert!(take_written().is_empty());
    }

    /// A program remembered as device-addressed is told apart by its address and its bytes: the
    /// same bytes elsewhere, or other bytes at the same address, are not it.
    #[test]
    fn a_device_addressed_program_is_remembered_by_address_and_bytes() {
        let (address, bytes) = (0x7400_5b85_4a00_u64, [1_u8, 2, 3, 4]);
        assert!(!goes_device_addressed(address, &bytes));
        remember_device_addressed(address, &bytes);
        assert!(goes_device_addressed(address, &bytes));
        assert!(!goes_device_addressed(address + 0x100, &bytes));
        assert!(!goes_device_addressed(address, &[1, 2, 3, 5]));
    }

    /// The refusals for what the host cannot know are told from the rest.
    #[test]
    fn a_refusal_the_host_cannot_know_is_told_apart() {
        assert!(host_cannot_know(
            "a jump leaves a block a branch on an execution mask the host cannot know may skip (D758)"
        ));
        assert!(!host_cannot_know(
            "two buffers the dispatch binds overlap, one of them written (D746, D953)"
        ));
    }
}
