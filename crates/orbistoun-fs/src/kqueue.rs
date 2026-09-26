//! Waiting for many descriptors to be ready: `kqueue` and `kevent`.
//!
//! A server with a listener and many clients makes a queue, registers what it wants to hear
//! about, and asks the queue what happened. `struct kevent` has two shapes in
//! `sys/sys/event.h`: the current 64-byte one with `ext[4]` at 32, and the 32-byte
//! `freebsd11_kevent`. Everything up to `udata` is at the same offset in both. The choice is
//! the same setting [`crate::metadata`] uses for `stat` and `dirent`, since all three changed
//! at one ABI boundary (D374).
//!
//! `EVFILT_READ` and `EVFILT_WRITE` on a descriptor are served; every other filter is
//! refused, since a timer that never fires would park a guest forever. Readiness is polled
//! through the descriptor table exactly as [`crate::select`] polls it, and asking never
//! consumes.

use std::collections::BTreeMap;

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// What a call answers when it did not work.
const FAILED: u64 = -1_i64 as u64;

/// How long to wait between asking every registration again.
///
/// A millisecond, as [`crate::select`] uses.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1);

/// The numbers this module tests against, read from the harvested `sys/sys/event.h`.
///
/// Resolved once as a whole: a name missing from the table would read as zero, which is no
/// filter, so every comparison would fail to match and registrations would never fire. If
/// any is missing this build refuses `kqueue`, saying so once.
#[derive(Debug, Clone, Copy)]
struct Numbers {
    /// `EVFILT_READ`.
    read: i16,
    /// `EVFILT_WRITE`.
    write: i16,
    /// `EV_ADD`.
    add: u16,
    /// `EV_DELETE`.
    delete: u16,
    /// `EV_ENABLE`.
    enable: u16,
    /// `EV_DISABLE`.
    disable: u16,
    /// `EV_ONESHOT`.
    oneshot: u16,
    /// `EV_RECEIPT`.
    receipt: u16,
    /// `EV_ERROR`.
    error: u16,
    /// `EINVAL`, for a filter nothing here reports on.
    invalid: i64,
    /// `ENOENT`, for a registration that is not there.
    missing: i64,
}

impl Numbers {
    /// Reads them, or nothing when the harvested table cannot name one.
    fn read_from_table() -> Option<Self> {
        let filter = |name: &str| {
            orbistoun_hle::constants::abi_constant("event", name)
                .and_then(|v| i16::try_from(v).ok())
        };
        let flag = |name: &str| {
            orbistoun_hle::constants::abi_constant("event", name)
                .and_then(|v| u16::try_from(v).ok())
        };
        Some(Self {
            read: filter("EVFILT_READ")?,
            write: filter("EVFILT_WRITE")?,
            add: flag("EV_ADD")?,
            delete: flag("EV_DELETE")?,
            enable: flag("EV_ENABLE")?,
            disable: flag("EV_DISABLE")?,
            oneshot: flag("EV_ONESHOT")?,
            receipt: flag("EV_RECEIPT")?,
            error: flag("EV_ERROR")?,
            invalid: orbistoun_hle::constants::abi_constant("errno", "EINVAL")?,
            missing: orbistoun_hle::constants::abi_constant("errno", "ENOENT")?,
        })
    }

    /// What this run resolved, said once if it resolved nothing.
    fn get() -> Option<Self> {
        static RESOLVED: std::sync::OnceLock<Option<Numbers>> = std::sync::OnceLock::new();
        *RESOLVED.get_or_init(|| {
            let read = Self::read_from_table();
            if read.is_none() {
                tracing::warn!(
                    "this build cannot name the event filters, so kqueue is refused - the harvested sys/sys/event.h is incomplete"
                );
            }
            read
        })
    }

    /// Whether a filter is one this reports on.
    const fn serves(self, which: i16) -> bool {
        which == self.read || which == self.write
    }
}

/// What a guest asked one queue to watch, keyed the way the interface keys it.
///
/// `(ident, filter)` identifies a registration, so one descriptor registered for reading and
/// for writing is two entries.
pub(crate) type Registrations = BTreeMap<(u64, i16), Registration>;

/// One thing a queue is watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Registration {
    /// The opaque word the guest attached, handed straight back in every report.
    udata: u64,
    /// Whether it is currently reported on.
    enabled: bool,
    /// Whether it is removed after being reported once.
    once: bool,
}

/// A `struct kevent`, in whichever of the two shapes this run uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Event {
    /// What the event is about: a descriptor, for the two filters served here.
    ident: u64,
    /// Which filter.
    filter: i16,
    /// What to do, or what happened.
    flags: u16,
    /// Filter-specific flags. Nothing here sets any.
    fflags: u32,
    /// Filter-specific data. For an error report, the `errno`.
    data: i64,
    /// The guest's own word.
    udata: u64,
}

impl Event {
    /// Bytes one occupies in this run's shape.
    fn len() -> usize {
        match crate::metadata::Layout::configured() {
            crate::metadata::Layout::FreeBsd11 => 32,
            crate::metadata::Layout::Current => 64,
        }
    }

    /// The bytes before `ext`, which is the whole of the shorter shape and the head of the
    /// longer one.
    const HEAD: usize = 32;

    /// Reads the one at `address`.
    ///
    /// Copied once into a local array and decoded safely, so there is one unsafe operation.
    ///
    /// # Safety
    ///
    /// `address` must point at a `struct kevent` in guest memory, the same contract the real
    /// call has under the identity mapping.
    unsafe fn read(address: u64) -> Option<Self> {
        let at = usize::try_from(address).ok()?;
        if at == 0 {
            return None;
        }
        let mut head = [0_u8; Self::HEAD];
        // SAFETY: the caller guarantees a `struct kevent` here, whose shorter shape is
        // exactly `HEAD` bytes and whose longer one begins with them.
        unsafe {
            std::ptr::copy_nonoverlapping(
                std::ptr::with_exposed_provenance::<u8>(at),
                head.as_mut_ptr(),
                Self::HEAD,
            );
        }
        // Every slice below is a fixed range of a fixed-size array, so the conversions cannot
        // fail.
        let eight = |from: usize| -> [u8; 8] { head[from..from + 8].try_into().unwrap_or([0; 8]) };
        let four = |from: usize| -> [u8; 4] { head[from..from + 4].try_into().unwrap_or([0; 4]) };
        let two = |from: usize| -> [u8; 2] { head[from..from + 2].try_into().unwrap_or([0; 2]) };
        Some(Self {
            ident: u64::from_le_bytes(eight(0)),
            filter: i16::from_le_bytes(two(8)),
            flags: u16::from_le_bytes(two(10)),
            fflags: u32::from_le_bytes(four(12)),
            data: i64::from_le_bytes(eight(16)),
            udata: u64::from_le_bytes(eight(24)),
        })
    }

    /// Writes it at `address`, filling the whole structure this run's shape defines.
    ///
    /// The `ext[4]` tail is zeroed rather than left as the caller's uninitialised bytes.
    ///
    /// # Safety
    ///
    /// `address` must point at [`Event::len`] writable bytes in guest memory.
    unsafe fn write(self, address: u64) {
        let Ok(at) = usize::try_from(address) else {
            return;
        };
        if at == 0 {
            return;
        }
        // Built whole, including the zeroed `ext` tail, and copied in one go.
        let mut bytes = [0_u8; 64];
        bytes[0..8].copy_from_slice(&self.ident.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.filter.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.flags.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.fflags.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.data.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.udata.to_le_bytes());
        let width = Self::len();
        // SAFETY: the caller guarantees `Event::len()` writable bytes, which is what is
        // written: 32 or 64, both within `bytes`.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                std::ptr::with_exposed_provenance_mut::<u8>(at),
                width,
            );
        }
    }
}

/// `kqueue()`: a new event queue.
///
/// Answers a descriptor, from the same table files and sockets come from, so `close` on it
/// works without knowing what it is.
///
/// Reference: FreeBSD `kqueue(2)`.
fn kqueue(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if Numbers::get().is_none() {
        return FAILED;
    }
    crate::descriptor::insert_queue().unwrap_or(FAILED)
}

/// How long a `kevent` may wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// Until something is ready, however long that takes.
    Forever,
    /// This long, which may be no time at all.
    Until(std::time::Duration),
}

/// Reads the `timespec` a caller passed, or [`Wait::Forever`] for a null one.
///
/// A null timeout means block; a zeroed one means ask once and answer. This takes a
/// `timespec` (seconds and nanoseconds), where `select` takes a `timeval`.
///
/// # Safety
///
/// `address`, when non-null, must point at a `struct timespec` in guest memory.
unsafe fn read_timeout(address: u64) -> Wait {
    let Ok(at) = usize::try_from(address) else {
        return Wait::Forever;
    };
    if at == 0 {
        return Wait::Forever;
    }
    let base = std::ptr::with_exposed_provenance::<u64>(at);
    // SAFETY: the caller guarantees a `timespec` here, two machine words from
    // `sys/sys/_timespec.h`.
    let seconds = unsafe { std::ptr::read_unaligned(base) };
    // SAFETY: the second field of the same structure.
    let nanos_at = unsafe { base.add(1) };
    // SAFETY: in bounds by the line above.
    let nanos = unsafe { std::ptr::read_unaligned(nanos_at) };
    Wait::Until(std::time::Duration::from_secs(seconds) + std::time::Duration::from_nanos(nanos))
}

/// Applies one change to a queue's registrations.
///
/// Answers the `errno` to report against this change, or zero when it was accepted. A bad
/// change does not stop the ones after it: each failure is reported as its own `EV_ERROR`
/// event.
fn apply(numbers: Numbers, held: &mut Registrations, change: Event) -> i64 {
    let key = (change.ident, change.filter);
    if !numbers.serves(change.filter) {
        // A filter this never reports on is a wakeup the guest waits for and never gets.
        return numbers.invalid;
    }
    if change.flags & numbers.delete != 0 {
        return if held.remove(&key).is_some() {
            0
        } else {
            numbers.missing
        };
    }
    if change.flags & numbers.add != 0 {
        held.insert(
            key,
            Registration {
                udata: change.udata,
                enabled: change.flags & numbers.disable == 0,
                once: change.flags & numbers.oneshot != 0,
            },
        );
        return 0;
    }
    let Some(entry) = held.get_mut(&key) else {
        return numbers.missing;
    };
    if change.flags & numbers.enable != 0 {
        entry.enabled = true;
    }
    if change.flags & numbers.disable != 0 {
        entry.enabled = false;
    }
    0
}

/// Whether a registration would report right now.
///
/// Asked of the descriptor table exactly as `select` asks it, so the two agree on "ready".
fn ready(numbers: Numbers, ident: u64, which: i16) -> bool {
    if crate::descriptor::is_standard(ident) {
        return true;
    }
    if which == numbers.read {
        return crate::descriptor::readable(ident);
    }
    if which == numbers.write {
        return crate::descriptor::writable(ident);
    }
    false
}

/// `kevent(kq, changelist, nchanges, eventlist, nevents, timeout)`.
///
/// Applies the changes, then waits for something registered to be ready and reports it.
/// Answers how many events were written, zero for a timeout, `-1` for a descriptor that is
/// not a queue.
///
/// The changes are applied before the wait and their failures reported first, as the
/// interface documents.
///
/// Reference: FreeBSD `kevent(2)`.
fn kevent(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (queue, changes_at, change_count, events_at, event_count, timeout_at) =
        (args[0], args[1], args[2], args[3], args[4], args[5]);

    let Some(numbers) = Numbers::get() else {
        return FAILED;
    };
    let stride = Event::len() as u64;
    let mut reports: Vec<Event> = Vec::new();

    // Every change, in order, each failure carried back as its own event.
    for index in 0..change_count {
        // SAFETY: a guest-supplied array of `nchanges` `struct kevent` under the identity
        // mapping, read within the count the guest declared.
        let Some(change) = (unsafe { Event::read(changes_at + index * stride) }) else {
            break;
        };
        let Some(failure) =
            crate::descriptor::with_queue(queue, |held| apply(numbers, held, change))
        else {
            // Not a queue at all: the caller's mistake, not a per-change one.
            return FAILED;
        };
        if failure != 0 || change.flags & numbers.receipt != 0 {
            reports.push(Event {
                flags: numbers.error,
                data: failure,
                ..change
            });
        }
    }

    if event_count == 0 {
        // Only registering: the changes are applied and there is nowhere to report to.
        return 0;
    }

    // SAFETY: a guest-supplied `timespec`, or null for "wait as long as it takes".
    let wait = unsafe { read_timeout(timeout_at) };
    let started = std::time::Instant::now();
    loop {
        // Copied out before anything is asked: testing readiness takes the descriptor table's
        // lock, and holding a queue's lock across that would take two locks in two orders.
        let Some(watching) = crate::descriptor::with_queue(queue, |held| {
            held.iter()
                .filter(|(_, entry)| entry.enabled)
                .map(|((ident, which), entry)| (*ident, *which, *entry))
                .collect::<Vec<_>>()
        }) else {
            return FAILED;
        };

        let room = usize::try_from(event_count).unwrap_or(usize::MAX);
        let mut fired: Vec<Event> = Vec::new();
        for (ident, which, entry) in watching {
            if reports.len() + fired.len() >= room {
                break;
            }
            if ready(numbers, ident, which) {
                fired.push(Event {
                    ident,
                    filter: which,
                    flags: 0,
                    fflags: 0,
                    // How much is waiting cannot be known without consuming it (D373), so zero
                    // is reported.
                    data: 0,
                    udata: entry.udata,
                });
                if entry.once {
                    let _ =
                        crate::descriptor::with_queue(queue, |held| held.remove(&(ident, which)));
                }
            }
        }

        let expired = match wait {
            Wait::Forever => false,
            Wait::Until(limit) => started.elapsed() >= limit,
        };
        if !fired.is_empty() || !reports.is_empty() || expired {
            reports.extend(fired);
            for (index, event) in reports.iter().enumerate() {
                // SAFETY: a guest-supplied array of `nevents` `struct kevent`, written within
                // the count the guest declared; the loop above never collects more.
                unsafe { event.write(events_at + index as u64 * stride) };
            }
            return reports.len() as u64;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Implementations this module provides, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("kqueue", kqueue), ("kevent", kevent)]
}

#[cfg(test)]
mod tests {
    use super::{Event, Numbers, Registrations, apply};

    /// The numbers, or a failure that says the harvested table is incomplete.
    fn numbers() -> Numbers {
        Numbers::get().expect("the harvested sys/sys/event.h names every filter and flag")
    }

    /// The filters are harvested and present (D385).
    #[test]
    fn the_filters_come_from_the_header() {
        let numbers = numbers();
        assert_eq!(numbers.read, -1, "EVFILT_READ");
        assert_eq!(numbers.write, -2, "EVFILT_WRITE");
        assert_eq!(numbers.add, 0x0001, "EV_ADD");
        assert_eq!(numbers.error, 0x4000, "EV_ERROR");
    }

    /// An event round-trips through guest memory at the offsets the header states.
    #[test]
    fn an_event_reads_back_as_it_was_written() {
        let numbers = numbers();
        let mut memory = [0_u8; 64];
        let at = memory.as_mut_ptr() as u64;
        let event = Event {
            ident: 7,
            filter: numbers.read,
            flags: numbers.add,
            fflags: 0,
            data: 0,
            udata: 0xDEAD_BEEF,
        };
        // SAFETY: `memory` is at least `Event::len()` bytes and lives for the test.
        unsafe { event.write(at) };
        // SAFETY: as above.
        let back = unsafe { Event::read(at) }.expect("a non-null address");
        assert_eq!(back, event);
        assert_eq!(memory[0], 7, "ident is the first field");
    }

    /// Adding, disabling and deleting are three different things.
    #[test]
    fn a_change_list_is_applied_the_way_the_interface_says() {
        let numbers = numbers();
        let mut held = Registrations::new();
        let add = Event {
            ident: 3,
            filter: numbers.read,
            flags: numbers.add,
            udata: 99,
            ..Event::default()
        };
        assert_eq!(apply(numbers, &mut held, add), 0);
        assert_eq!(held.len(), 1);
        assert!(held[&(3, numbers.read)].enabled);

        let disable = Event {
            flags: numbers.disable,
            ..add
        };
        assert_eq!(apply(numbers, &mut held, disable), 0);
        assert!(
            !held[&(3, numbers.read)].enabled,
            "still registered, not reported"
        );

        let delete = Event {
            flags: numbers.delete,
            ..add
        };
        assert_eq!(apply(numbers, &mut held, delete), 0);
        assert!(held.is_empty());
        assert_ne!(
            apply(numbers, &mut held, delete),
            0,
            "deleting what is not there is an error, not a no-op"
        );
    }

    /// A filter nothing reports on is refused rather than accepted and never fired.
    #[test]
    fn an_unserved_filter_is_refused() {
        let numbers = numbers();
        let timer = orbistoun_hle::constants::abi_constant("event", "EVFILT_TIMER")
            .and_then(|v| i16::try_from(v).ok())
            .expect("the header names it");
        let mut held = Registrations::new();
        let change = Event {
            ident: 1,
            filter: timer,
            flags: numbers.add,
            ..Event::default()
        };
        assert_ne!(apply(numbers, &mut held, change), 0);
        assert!(held.is_empty(), "and nothing is remembered about it");
    }

    /// The same descriptor registered two ways is two registrations.
    #[test]
    fn reading_and_writing_are_separate_registrations() {
        let numbers = numbers();
        let mut held = Registrations::new();
        let base = Event {
            ident: 4,
            flags: numbers.add,
            ..Event::default()
        };
        apply(
            numbers,
            &mut held,
            Event {
                filter: numbers.read,
                ..base
            },
        );
        apply(
            numbers,
            &mut held,
            Event {
                filter: numbers.write,
                ..base
            },
        );
        assert_eq!(held.len(), 2);
    }
}
