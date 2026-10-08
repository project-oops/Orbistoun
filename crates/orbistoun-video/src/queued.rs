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

/// The filter of the event the graphics driver posts for an interrupting release: `EVFILT_AGC`,
/// read on hardware as -14 (obSCEne REQ-20261008T1200Z-eo03, `166-agc/driver-add-eq-event`, sweep
/// 20261008-142306).
const EVFILT_AGC: i16 = -14;
/// Its flags word, `0x20` (`EV_CLEAR`) on hardware (`-eo03`).
const AGC_EVENT_FLAGS: u16 = 0x20;
/// The low sixteen bits of its `data`, `0xff00` on hardware for both context ids tried (`-eo03`:
/// `0` and `0x101`), the release's context id above them.
const AGC_EVENT_TYPE: u64 = 0xff00;

/// `sceAgcDriverAddEqEvent(queue, id, udata)`: registers the driver's end-of-pipe event `id`
/// against an event queue (D762), answering whether the queue exists.
#[must_use]
pub fn register_end_of_pipe(queue: u64, id: u64, udata: u64) -> bool {
    orbistoun_kernel::sync::register_filtered_event(queue, EVFILT_AGC, id, udata)
}

/// Posts the driver's end-of-pipe event to every queue registered for it (D762), as hardware posts
/// it for a retired interrupting release (`-eo03`, `kevent-32b`): `ident` the registered id,
/// `filter` `EVFILT_AGC`, `flags` `AGC_EVENT_FLAGS`, `fflags` 0, `data` the release's context
/// id above `AGC_EVENT_TYPE`, `udata` the registration's. Answers how many queues took one.
pub fn post_end_of_pipe(context: u32) -> usize {
    let data = (u64::from(context) << 16) | AGC_EVENT_TYPE;
    orbistoun_kernel::sync::post_filtered_event(EVFILT_AGC, |id| {
        orbistoun_kernel::sync::PendingEvent {
            ident: id,
            filter: EVFILT_AGC,
            flags: AGC_EVENT_FLAGS,
            fflags: 0,
            data: i64::from_ne_bytes(data.to_ne_bytes()),
            udata: 0,
        }
    })
}

/// The command processor released `label` with an interrupt whose context id is `context`. The
/// driver's end-of-pipe event goes to every queue registered for it (D762). When the release is a
/// queued flip, the flip is performed and the label of the buffer it replaced on screen is cleared;
/// anything else is not a flip and is left alone. Answers whether a flip was performed.
pub fn released(label: u64, context: u32) -> bool {
    post_end_of_pipe(context);
    let queued = {
        let mut waiting = pending();
        match waiting.get(&context) {
            Some(q) if q.label == label => waiting.remove(&context),
            _ => None,
        }
    };
    let Some(q) = queued else {
        return false;
    };
    let replaced = on_screen().insert(q.handle, q.index);
    if let Some(previous) = replaced.filter(|&previous| previous != q.index) {
        // SAFETY: a label in the region `queue` mapped before any flip could be queued.
        unsafe { guest::write_u64(label_of(previous), 0) };
    }
    crate::flip(q.handle, q.index, q.arg) == 0
}

#[cfg(test)]
mod end_of_pipe_tests {
    /// An interrupting release posts what hardware posted for one (obSCEne REQ-20261008T1200Z-eo03,
    /// sweep 20261008-142306, `arm3-eo03-helper-release-ev0`): with event 0 registered under
    /// `udata` `0x11223344`, a release under context id `0x101` delivers exactly these 32 bytes.
    #[test]
    fn an_interrupting_release_posts_the_measured_event() {
        let queue = orbistoun_kernel::sync::create_equeue("end of pipe");
        assert!(super::register_end_of_pipe(queue, 0, 0x1122_3344));
        assert!(
            !super::released(0x7400_0000_1000, 0x101),
            "no flip was queued"
        );
        // Other tests release concurrently into the same process's queues, so this one's event is
        // looked for among them.
        let taken: Vec<_> = orbistoun_kernel::sync::take_events(queue, 16)
            .into_iter()
            .map(orbistoun_kernel::sync::PendingEvent::to_bytes)
            .collect();
        let measured: [u8; 32] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0xf2, 0xff, 0x20, 0, 0, 0, 0, 0, //
            0, 0xff, 0x01, 0x01, 0, 0, 0, 0, 0x44, 0x33, 0x22, 0x11, 0, 0, 0, 0,
        ];
        assert!(
            taken.iter().any(|bytes| bytes[..] == measured),
            "{taken:x?}"
        );
    }
}
