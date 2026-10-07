//! Flips a command buffer carries: `sceAgcDcbSetFlip` queues one, and it is carried out where the
//! stream releases the buffer's flip label (D728).
//!
//! On hardware the builder writes the display's flip registers, the flip argument, and an
//! end-of-pipe `RELEASE_MEM` that stores `1` into the buffer's label with an interrupt whose
//! context id names the flip (obSCEne `-1d54`, `166-agc/dcb-set-flip`, sweep 20260927-013000 lines
//! 12923-12975). The labels sit at [`FLIP_LABEL_BASE`] plus eight bytes per buffer, and
//! `sceAgcDriverWaitUntilSafeForRendering` waits for a buffer's label to read `0` again before
//! drawing into it (`-354a`, lines 6402-6452): the display clears a label when its buffer leaves
//! the screen.
//!
//! Here the builder records the flip under the context id it encoded; when the command processor
//! reaches the release it reports the label and context id, and [`released`] performs the flip
//! through the same path as `sceVideoOutSubmitFlip` and clears the label of the buffer it replaced.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use orbistoun_mem::guest;

/// The first buffer's flip label: `0xC_8000_40A0` on hardware, eight bytes per buffer after it
/// (`-1d54`: buffers 0 and 1 released to `0xC_8000_40A0` and `0xC_8000_40A8`). The console's own
/// address, not one this project chose; one port was observed.
pub const FLIP_LABEL_BASE: u64 = 0x0000_000C_8000_40A0;

/// The mapping that holds the labels: the host's 64 KiB reservation granule around them.
const LABEL_REGION: (u64, u64) = (0x0000_000C_8000_0000, 0x1_0000);

/// The interrupt context id of the first flip a process queues, `+1` per flip after it (`-1d54`:
/// `0x08000101` to `0x08000104` across four calls).
const FIRST_CONTEXT: u32 = 0x0800_0101;

/// The next interrupt context id to hand out.
static NEXT_CONTEXT: AtomicU32 = AtomicU32::new(FIRST_CONTEXT);

/// A flip waiting for its release.
#[derive(Debug, Clone, Copy)]
struct Queued {
    handle: u64,
    index: u64,
    arg: u64,
    label: u64,
}

fn pending() -> MutexGuard<'static, HashMap<u32, Queued>> {
    static PENDING: OnceLock<Mutex<HashMap<u32, Queued>>> = OnceLock::new();
    PENDING
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The buffer each port shows, by port handle.
fn on_screen() -> MutexGuard<'static, HashMap<u64, u64>> {
    static SHOWN: OnceLock<Mutex<HashMap<u64, u64>>> = OnceLock::new();
    SHOWN
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Buffer `index`'s flip label.
#[must_use]
pub const fn label_of(index: u64) -> u64 {
    FLIP_LABEL_BASE + index * 8
}

/// Queues a flip of `index` on port `handle` with `arg`, answering the interrupt context id and the
/// label its release must carry - or `None`, as hardware writes nothing for a port that is not
/// open or a buffer index that is not registered (`-1d54`: index 4 with two registered, and the
/// unopened control arm).
#[must_use]
pub fn queue(handle: u64, index: u64, arg: u64) -> Option<(u32, u64)> {
    let registered = crate::port::with(handle, |p| index < p.buffers.len() as u64)?;
    if !registered {
        return None;
    }
    let label = label_of(index);
    if label + 8 > LABEL_REGION.0 + LABEL_REGION.1
        || !orbistoun_kernel::map_system_region(LABEL_REGION.0, LABEL_REGION.1)
    {
        return None;
    }
    let context = NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed);
    pending().insert(
        context,
        Queued {
            handle,
            index,
            arg,
            label,
        },
    );
    Some((context, label))
}

/// The label a wait for buffer `index` of port `handle` to leave the screen polls, with the page
/// that holds it mapped so the command processor can read it - or `None` when the port is not open.
/// The buffer need not be registered: `-5a17` wrote the wait for index 4 as for 0 and 1.
#[must_use]
pub fn wait_label(handle: u64, index: u64) -> Option<u64> {
    crate::port::with(handle, |_| ())?;
    let label = label_of(index);
    (label + 8 <= LABEL_REGION.0 + LABEL_REGION.1
        && orbistoun_kernel::map_system_region(LABEL_REGION.0, LABEL_REGION.1))
    .then_some(label)
}

/// The command processor released `label` with interrupt context id `context`. When that is a
/// queued flip, the flip is performed and the label of the buffer it replaced on screen is cleared;
/// anything else is not a flip and is left alone. Answers whether a flip was performed.
pub fn released(label: u64, context: u32) -> bool {
    let queued = {
        let mut waiting = pending();
        match waiting.get(&context) {
            Some(q) if q.label == label => waiting.remove(&context),
            _ => None,
        }
    };
    let Some(q) = queued else {
        // `ORBISTOUN_EOP_TO_ALL` asks what a title blocked on its `sceAgcDriverAddEqEvent` queue
        // does when that wait completes. It is off by default and intervenes (D227).
        if std::env::var_os(orbistoun_env::EOP_TO_ALL.name).is_some() {
            post_eop_completion(label, context);
        }
        return false;
    };
    let replaced = on_screen().insert(q.handle, q.index);
    if let Some(previous) = replaced.filter(|&previous| previous != q.index) {
        // SAFETY: a label in the region `queue` mapped before any flip could be queued.
        unsafe { guest::write_u64(label_of(previous), 0) };
    }
    crate::flip(q.handle, q.index, q.arg) == 0
}

/// Posts an end-of-pipe completion to every event queue, answering how many took it.
///
/// Only for the `ORBISTOUN_EOP_TO_ALL` diagnostic. What the driver posts for a retired
/// interrupting release is unmeasured (obSCEne -eo01), so this models nothing: the release's
/// context id is the `ident` and its label the `data`, which only lets a reader tell one post from
/// another.
pub fn post_eop_completion(label: u64, context: u32) -> usize {
    orbistoun_kernel::sync::post_event_everywhere(orbistoun_kernel::sync::PendingEvent {
        ident: u64::from(context),
        filter: 0,
        flags: 0,
        fflags: 0,
        data: i64::from_ne_bytes(label.to_ne_bytes()),
        udata: 0,
    })
}

#[cfg(test)]
mod tests {
    /// Under `ORBISTOUN_EOP_TO_ALL`, an interrupting release that is no flip wakes every queue: a
    /// title blocked on its `sceAgcDriverAddEqEvent` queue gets an event, its context id as `ident`
    /// and its label as `data`.
    #[test]
    fn an_end_of_pipe_completion_reaches_every_queue() {
        let queue = orbistoun_kernel::sync::create_equeue("eop experiment");
        assert!(super::post_eop_completion(0x7400_00c9_c610, 0x800_0101) >= 1);
        let taken = orbistoun_kernel::sync::take_events(queue, 4);
        assert_eq!(taken.len(), 1);
        assert_eq!(
            (taken[0].ident, taken[0].data),
            (0x800_0101, 0x7400_00c9_c610)
        );
    }
}
