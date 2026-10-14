//! The wait sub-loop's second and third steps, run while the guest runs (D832).
//!
//! A thread waiting in a loop around one call reads a word nothing writes. The first step names
//! the loop and the objects its callee-saved registers hold (`orbistoun_report::trace::Wait`). This
//! one watches those objects a few words at a time, on every guest thread, until a read lands next
//! to the waiting call: that is the loop's read, and the word it read is the word it waits on. Then
//! it watches that word for writes. The guest's code is never read: a watchpoint says where an
//! access came from, and nearness to the call site says which access was the loop's (D276).
//!
//! In-run because a waiting object's address is not the same from one run to the next.

use std::sync::Mutex;
use std::time::Duration;

use crate::watchpoint::{self, Kind, MAX_WATCHPOINTS, Request};

/// How long each set of watches is left armed before its hits are read.
const TICK: Duration = Duration::from_millis(250);

/// How far into an object a register points at the loop's word is looked for.
pub const OBJECT_BYTES: u64 = 0x100;

/// How near the waiting call, in either direction, an access must be to be the loop's own: a loop
/// around a call reads its word just before the call or just after it returns.
const LOOP_REACH: u64 = 0x80;

/// How many ticks the word is watched for writes.
const WRITE_TICKS: u32 = 8;

/// A thread waiting in a loop around one call, as the call ring shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waiting {
    /// The host thread, as the ring records it.
    pub host: u64,
    /// The waiting call's return address.
    pub site: u64,
    /// The callee-saved registers at its newest call, in `CALLEE_SAVED` order.
    pub saved: [u64; orbistoun_thunk::CALLEE_SAVED_REGISTERS],
}

/// The word a waiting loop reads, and who wrote it while it was watched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The waiting thread.
    pub host: u64,
    /// The waiting call's return address.
    pub site: u64,
    /// The word, when a watched read landed next to the call.
    pub word: Option<Word>,
}

/// Where a watched word came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The object a callee-saved register pointed at, by `CALLEE_SAVED` index.
    Register(usize),
    /// A write the stopped GPU queue never made: the word a thread waiting on the GPU polls.
    Held(orbistoun_gpu::cp::HeldWrite),
}

/// The word a waiting loop reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// Its address: an eight-byte word holding what the loop reads.
    pub address: u64,
    /// Where it came from.
    pub origin: Origin,
    /// The instruction after the loop's read.
    pub read_after: u64,
    /// The instructions after each write while it was watched; empty when nothing wrote it.
    pub writers: Vec<u64>,
}

/// What every waiting thread resolved to, in the order they were resolved.
static RESOLVED: Mutex<Vec<Resolved>> = Mutex::new(Vec::new());

/// What the waiting thread `host`, looping at `site`, resolved to, if it was resolved.
pub fn resolved(host: u64, site: u64) -> Option<Resolved> {
    RESOLVED
        .lock()
        .ok()?
        .iter()
        .find(|r| r.host == host && r.site == site)
        .cloned()
}

/// Every thread in the call ring waiting in a loop around one call.
#[must_use]
pub fn waiting(recorded: &[orbistoun_thunk::RecordedCall]) -> Vec<Waiting> {
    let mut hosts: Vec<u64> = recorded.iter().map(|call| call.thread).collect();
    hosts.sort_unstable();
    hosts.dedup();
    hosts
        .into_iter()
        .filter(|&host| host != 0)
        .filter_map(|host| {
            let own: Vec<&orbistoun_thunk::RecordedCall> =
                recorded.iter().filter(|call| call.thread == host).collect();
            let calls: Vec<(u32, u64)> = own.iter().map(|call| (call.index, call.from)).collect();
            let (_, site) = orbistoun_report::trace::waiting_loop(&calls)?;
            let newest = own.iter().max_by_key(|call| call.sequence)?;
            Some(Waiting {
                host,
                site,
                saved: newest.saved,
            })
        })
        .collect()
}

/// The words to watch for the loop's read: each register's object, eight bytes at a time, as
/// far as [`OBJECT_BYTES`], keeping only what `readable` allows, each word once.
#[must_use]
pub fn candidate_words(saved: &[u64], readable: &dyn Fn(u64) -> bool) -> Vec<(usize, u64)> {
    let mut out: Vec<(usize, u64)> = Vec::new();
    for (register, &value) in saved.iter().enumerate() {
        let base = value & !7;
        for offset in (0..OBJECT_BYTES).step_by(8) {
            let word = base.saturating_add(offset);
            if readable(word) && !out.iter().any(|&(_, seen)| seen == word) {
                out.push((register, word));
            }
        }
    }
    out
}

/// Every word to watch for the loop's read: the registers' objects ([`candidate_words`]), then the
/// word of each write the stopped GPU queue holds, newest first, each word once and readable.
#[must_use]
pub fn watch_list(
    saved: &[u64],
    held: &[orbistoun_gpu::cp::HeldWrite],
    readable: &dyn Fn(u64) -> bool,
) -> Vec<(Origin, u64)> {
    let mut out: Vec<(Origin, u64)> = candidate_words(saved, readable)
        .into_iter()
        .map(|(register, word)| (Origin::Register(register), word))
        .collect();
    for write in held.iter().rev() {
        let word = write.address & !7;
        if readable(word) && !out.iter().any(|&(_, seen)| seen == word) {
            out.push((Origin::Held(*write), word));
        }
    }
    out
}

/// How a call was handed a waiting loop's object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handed {
    /// An argument pointed into it.
    Argument,
    /// An argument pointed at memory holding a pointer into it, such as a callback's context.
    Pointee,
}

/// The calls that were handed the object `[base, base + len)`, among the argument dumps: the step
/// that names the call a waiting loop's word was to be completed by (D832). Each as
/// `(import, argument, how)`, in the order the dumps were taken.
#[must_use]
pub fn handed_to(
    base: u64,
    len: u64,
    dumps: &[orbistoun_thunk::ArgumentDump],
) -> Vec<(u32, u8, Handed)> {
    let inside = |value: u64| value >= base && value < base.saturating_add(len);
    let mut out: Vec<(u32, u8, Handed)> = Vec::new();
    for dump in dumps {
        let how = if inside(dump.address) {
            Handed::Argument
        } else if dump
            .bytes
            .chunks_exact(8)
            .filter_map(|word| word.try_into().ok().map(u64::from_le_bytes))
            .any(inside)
        {
            Handed::Pointee
        } else {
            continue;
        };
        let call = (dump.index, dump.slot, how);
        if !out.contains(&call) {
            out.push(call);
        }
    }
    out
}

/// Whether an access whose next instruction is `after` is the waiting loop's own, the loop being
/// around the call that returns to `site`.
#[must_use]
pub const fn is_the_loops(after: u64, site: u64) -> bool {
    after.abs_diff(site) <= LOOP_REACH
}

/// Starts resolving waiting threads in the background. Silent when the run armed watchpoints of
/// its own, which this would replace.
pub fn start() {
    if watchpoint::armed() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("orbistoun-waits".to_owned())
        .spawn(|| {
            loop {
                std::thread::sleep(TICK);
                let ring = orbistoun_thunk::recorded_calls();
                let next = waiting(&ring)
                    .into_iter()
                    .find(|w| resolved(w.host, w.site).is_none());
                if let Some(target) = next {
                    let outcome = resolve(target);
                    if let Ok(mut all) = RESOLVED.lock() {
                        all.push(outcome);
                    }
                }
            }
        });
}

/// Whether the word at `word` can be watched: it is committed, readable memory.
#[cfg(windows)]
fn readable(word: u64) -> bool {
    crate::report::readable(word, 8)
}

/// Away from Windows nothing here can watch memory, so no word is offered.
#[cfg(not(windows))]
const fn readable(_word: u64) -> bool {
    false
}

/// Whether `target` is still waiting where it was.
fn still_waiting(target: Waiting) -> bool {
    waiting(&orbistoun_thunk::recorded_calls())
        .iter()
        .any(|w| w.host == target.host && w.site == target.site)
}

/// Finds the word `target` waits on, then who writes it.
fn resolve(target: Waiting) -> Resolved {
    let words = watch_list(
        &target.saved,
        &orbistoun_gpu::agc_driver::held_writes(),
        &readable,
    );
    tracing::info!(
        "a thread waits at {:#x}: watching {} word(s) of its objects for the loop's read",
        target.site,
        words.len()
    );
    let mut found = None;
    for batch in words.chunks(MAX_WATCHPOINTS) {
        let requests: Vec<Request> = batch
            .iter()
            .map(|&(_, word)| Request {
                address: word,
                length: 8,
                kind: Kind::Access,
            })
            .collect();
        if watchpoint::rearm(&requests) == 0 {
            tracing::warn!(
                "no guest thread could be armed, so the wait at {:#x} stays unresolved",
                target.site
            );
            break;
        }
        std::thread::sleep(TICK);
        let hit = watchpoint::hits()
            .into_iter()
            .find(|hit| is_the_loops(hit.after, target.site));
        if let Some(hit) = hit {
            let (origin, _) = batch
                .iter()
                .copied()
                .find(|&(_, word)| word == hit.address)
                .unwrap_or((Origin::Register(0), hit.address));
            found = Some((origin, hit.address, hit.after));
            break;
        }
        if !still_waiting(target) {
            break;
        }
    }
    let word = found.map(|(origin, address, read_after)| {
        watchpoint::rearm(&[Request {
            address,
            length: 8,
            kind: Kind::Write,
        }]);
        std::thread::sleep(TICK * WRITE_TICKS);
        // The loop's own read is never a writer, whatever a trap still in flight from the read
        // watch recorded.
        let writers = watchpoint::hits()
            .iter()
            .map(|hit| hit.after)
            .filter(|&after| after != read_after)
            .collect();
        Word {
            address,
            origin,
            read_after,
            writers,
        }
    });
    watchpoint::rearm(&[]);
    if let Some(word) = &word {
        tracing::info!(
            "the wait at {:#x} reads {:#x}; {} write(s) while watched",
            target.site,
            word.address,
            word.writers.len()
        );
    } else {
        tracing::info!(
            "no watched word was read next to the wait at {:#x}",
            target.site
        );
    }
    Resolved {
        host: target.host,
        site: target.site,
        word,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Handed, OBJECT_BYTES, Origin, candidate_words, handed_to, is_the_loops, watch_list,
    };
    use orbistoun_thunk::{ArgumentDump, Pointing};

    /// A dump of argument `slot` of import `index` at `address`, holding `words`.
    fn dump(index: u32, slot: u8, address: u64, words: [u64; 4]) -> ArgumentDump {
        let mut bytes = [0_u8; orbistoun_thunk::DUMP_BYTES];
        for (at, word) in words.iter().enumerate() {
            bytes[at * 8..at * 8 + 8].copy_from_slice(&word.to_le_bytes());
        }
        ArgumentDump {
            index,
            slot,
            address,
            pointing: Pointing::Mapped,
            bytes,
            returned: None,
        }
    }

    /// A call handed the object, or a pointer to memory holding a pointer into it, is named; one
    /// handed neither is not, and a call is named once however often it was dumped.
    #[test]
    fn the_calls_handed_the_object_are_named() {
        let base = 0x7400_0610_b2c0;
        let dumps = [
            dump(3, 1, base + 0x10, [0; 4]),
            dump(4, 2, 0x6000_0080_0000, [0, base + 0x80, 0, 0]),
            dump(5, 0, 0x6000_0080_1000, [base + 0x100, 0, 0, 0]),
            dump(3, 1, base + 0x10, [0; 4]),
        ];
        assert_eq!(
            handed_to(base, 0x100, &dumps),
            vec![(3, 1, Handed::Argument), (4, 2, Handed::Pointee)],
            "the third points one past the object"
        );
    }

    /// Each register's object is watched a word at a time, unreadable words skipped and a word two
    /// registers share watched once.
    #[test]
    fn candidates_are_each_objects_readable_words_once() {
        let saved = [0x1000, 0, 0x1008, 0x2004];
        let readable = |word: u64| word >= 0x1000;
        let words = candidate_words(&saved, &readable);
        let per_object = (OBJECT_BYTES / 8) as usize;
        assert_eq!(words[0], (0, 0x1000));
        assert_eq!(words[per_object - 1], (0, 0x1000 + OBJECT_BYTES - 8));
        assert!(
            words.iter().all(|&(register, _)| register != 1),
            "a null register points at nothing readable"
        );
        assert_eq!(
            words.iter().filter(|&&(_, word)| word == 0x1008).count(),
            1,
            "a word two registers reach is watched once"
        );
        assert!(
            words.contains(&(3, 0x2000)),
            "an unaligned pointer starts at its word"
        );
    }

    /// After the registers' objects, the words the stopped GPU queue would have written are
    /// watched: a thread waiting on the GPU polls one (D832). Each word once, a register's claim
    /// first, and an unreadable one skipped.
    #[test]
    fn the_queues_held_writes_are_watched_after_the_objects() {
        let held = |address: u64| orbistoun_gpu::cp::HeldWrite {
            offset: 0x308,
            opcode: 0x49,
            address,
        };
        let readable = |word: u64| word >= 0x1000;
        let list = watch_list(
            &[0x1000],
            &[held(0x5004), held(0x1008), held(0x10), held(0x5000)],
            &readable,
        );
        let per_object = (OBJECT_BYTES / 8) as usize;
        assert_eq!(list.len(), per_object + 1);
        assert_eq!(
            list[1],
            (Origin::Register(0), 0x1008),
            "the register's claim"
        );
        assert_eq!(
            list[per_object],
            (Origin::Held(held(0x5000)), 0x5000),
            "the word once, aligned, claimed by the newest write to it"
        );
    }

    /// An access next to the waiting call is the loop's; one far from it is something else's.
    #[test]
    fn an_access_next_to_the_call_is_the_loops() {
        let site = 0x4000_02b5_f0e5;
        assert!(
            is_the_loops(0x4000_02b5_f0ed, site),
            "just after the call returns"
        );
        assert!(is_the_loops(0x4000_02b5_f0c5, site), "just before the call");
        assert!(
            !is_the_loops(0x4000_030e_b7cc, site),
            "elsewhere in the title"
        );
    }
}
