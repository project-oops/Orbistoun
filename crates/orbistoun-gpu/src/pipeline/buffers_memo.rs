//! A dispatch's traced buffers kept with the guest bytes they were worked out from (D981).
//!
//! Finding what a dispatch binds can mean running a sample of its waves on the host (D753, D914,
//! D951): seconds for a program whose stores go through a descriptor it computes. The answer is a
//! function of the program, the dispatch's entry state and the guest memory the work read, so it is
//! kept with what it read - the bytes of each span it read, and whether each span it only asked
//! after is mapped - and served again while memory still answers every one of them the same. A
//! byte read written since, or a span mapped or unmapped since, makes it work out afresh. The
//! queued writes of a submission and the command processor's preparation of the same dispatch then
//! pay for it once, not twice, each under the live pipeline, which every other preparation waits
//! for meanwhile.

use std::cell::RefCell;
use std::collections::VecDeque;

use super::GuestMemory;

/// How many answers are kept: a frame's dispatches that needed a host run, several times over.
const KEPT: usize = 64;

/// The most spans one answer is kept with: a run that read more, scattered, costs about as much to
/// check again as to make again.
const MOST_SPANS: usize = 1 << 16;

/// What one recorded access learned of a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Learned {
    /// Its bytes.
    Held,
    /// Only that it is mapped.
    Mapped,
    /// That it is not mapped, read or asked after.
    Unmapped,
}

/// Guest memory read through `memory`, every access recorded: what a kept answer depends on.
pub(super) struct Recording<'m, M: GuestMemory + ?Sized> {
    memory: &'m M,
    /// Each access, as `(what it learned, address, bytes)`, in the order made.
    accesses: RefCell<Vec<(Learned, u64, u64)>>,
}

impl<'m, M: GuestMemory + ?Sized> Recording<'m, M> {
    pub(super) const fn new(memory: &'m M) -> Self {
        Self {
            memory,
            accesses: RefCell::new(Vec::new()),
        }
    }

    fn note(&self, access: (Learned, u64, u64)) {
        let mut accesses = self.accesses.borrow_mut();
        // A host run reads one word many times over, wave after wave: kept once a run.
        if accesses.last() != Some(&access) {
            accesses.push(access);
        }
    }

    /// What the accesses learned, the spans read merged where they touch, with the hash of the
    /// bytes there now; `None` where those cannot be read back as one span each, or are too many
    /// to keep.
    pub(super) fn read_set(self) -> Option<ReadSet> {
        let mut accesses = self.accesses.into_inner();
        accesses.sort_unstable();
        accesses.dedup();
        let mut set = ReadSet::default();
        for (learned, address, bytes) in accesses {
            match learned {
                Learned::Held => match set.held.last_mut() {
                    Some((first, length)) if address <= *first + *length => {
                        *length = (*length).max(address.saturating_add(bytes) - *first);
                    }
                    _ => set.held.push((address, bytes)),
                },
                Learned::Mapped => set.mapped.push((address, bytes)),
                Learned::Unmapped => set.unmapped.push((address, bytes)),
            }
        }
        if set.held.len() + set.mapped.len() + set.unmapped.len() > MOST_SPANS {
            return None;
        }
        set.hash = hash_of(&set.held, self.memory)?;
        Some(set)
    }
}

impl<M: GuestMemory + ?Sized> GuestMemory for Recording<'_, M> {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        let read = self.memory.read(address, length);
        let learned = if read.is_some() {
            Learned::Held
        } else {
            Learned::Unmapped
        };
        self.note((learned, address, length as u64));
        read
    }

    fn mapped(&self, address: u64, length: usize) -> bool {
        let mapped = self.memory.mapped(address, length);
        let learned = if mapped {
            Learned::Mapped
        } else {
            Learned::Unmapped
        };
        self.note((learned, address, length as u64));
        mapped
    }
}

/// What an answer was worked out from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ReadSet {
    /// The spans whose bytes were read, merged where they touch, ascending.
    held: Vec<(u64, u64)>,
    /// The spans found mapped, their bytes not read.
    mapped: Vec<(u64, u64)>,
    /// The spans found not mapped.
    unmapped: Vec<(u64, u64)>,
    /// [`hash_of`] the held spans.
    hash: u64,
}

impl ReadSet {
    /// Whether `memory` answers as it answered when the set was made: every span read holds the
    /// same bytes, and every span asked after is mapped, or not, as it was.
    fn holds(&self, memory: &(impl GuestMemory + ?Sized)) -> bool {
        let mapped = |&(address, bytes): &(u64, u64)| {
            usize::try_from(bytes).is_ok_and(|bytes| memory.mapped(address, bytes))
        };
        self.mapped.iter().all(mapped)
            && !self.unmapped.iter().any(mapped)
            && hash_of(&self.held, memory) == Some(self.hash)
    }
}

/// A hash of the bytes at every span, each with where it lies; `None` where one does not read.
fn hash_of(spans: &[(u64, u64)], memory: &(impl GuestMemory + ?Sized)) -> Option<u64> {
    let mut hasher = crate::ContentHasher::new(spans.len());
    for &(address, bytes) in spans {
        let held = memory.read(address, usize::try_from(bytes).ok()?)?;
        hasher.word(address as u32);
        hasher.word((address >> 32) as u32);
        hasher.word(bytes as u32);
        hasher.bytes(held);
        // The bytes past the last whole word, which `bytes` leaves out.
        let whole = held.len() - held.len() % 4;
        let mut tail = [0u8; 4];
        tail[..held.len() - whole].copy_from_slice(&held[whole..]);
        hasher.word(u32::from_le_bytes(tail));
    }
    Some(hasher.finish())
}

/// Answers kept by what they were asked - `K`, compared whole - newest last.
#[derive(Debug)]
pub(super) struct BuffersMemo<K, V> {
    kept: std::sync::Mutex<VecDeque<(K, ReadSet, V)>>,
}

impl<K, V> Default for BuffersMemo<K, V> {
    fn default() -> Self {
        Self {
            kept: std::sync::Mutex::new(VecDeque::new()),
        }
    }
}

impl<K: PartialEq, V: Clone> BuffersMemo<K, V> {
    /// The answer kept for `key` where `memory` still answers what it was worked out from as it
    /// did; otherwise `work`'s, over `memory` recorded, kept for the next ask.
    pub(super) fn answer<M: GuestMemory + ?Sized>(
        &self,
        key: K,
        memory: &M,
        work: impl FnOnce(&Recording<'_, M>) -> V,
    ) -> V {
        let kept = self.kept.lock().ok().and_then(|kept| {
            kept.iter()
                .rev()
                .find(|(asked, _, _)| *asked == key)
                .map(|(_, reads, answer)| (reads.clone(), answer.clone()))
        });
        // Checked with the memo let go: a read can wait on the queue.
        if let Some((reads, answer)) = kept
            && reads.holds(memory)
        {
            return answer;
        }
        let recording = Recording::new(memory);
        let answer = work(&recording);
        if let Some(reads) = recording.read_set()
            && let Ok(mut kept) = self.kept.lock()
        {
            kept.retain(|(asked, _, _)| *asked != key);
            kept.push_back((key, reads, answer.clone()));
            while kept.len() > KEPT {
                kept.pop_front();
            }
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::{BuffersMemo, GuestMemory};
    use std::cell::Cell;

    /// Guest memory of one mapping of 256 bytes at 0x1000.
    struct Memory(Vec<u8>);

    impl Memory {
        fn new() -> Self {
            Self((0..=255).collect())
        }

        /// The same memory with the byte at `offset` set to `value`.
        fn with(&self, offset: usize, value: u8) -> Self {
            let mut bytes = self.0.clone();
            bytes[offset] = value;
            Self(bytes)
        }
    }

    impl GuestMemory for Memory {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            let at = usize::try_from(address.checked_sub(0x1000)?).ok()?;
            self.0.get(at..at.checked_add(length)?)
        }
    }

    /// The work a slow host run stands for: reads the byte at 0x1010, then the byte at 0x1000 plus
    /// that, and answers the second; asks whether the whole mapping is there, as a margin is
    /// checked; finds nothing at 0x9000; and counts how often it ran.
    fn chase(memory: &impl GuestMemory, ran: &Cell<usize>) -> Option<u8> {
        ran.set(ran.get() + 1);
        assert!(memory.mapped(0x1000, 256));
        assert!(memory.read(0x9000, 4).is_none());
        let first = *memory.read(0x1010, 1)?.first()?;
        memory.read(0x1000 + u64::from(first), 1)?.first().copied()
    }

    /// Asked again over the same memory, the answer is served without the work; a byte the work
    /// read written since makes it run again, and one it only asked was mapped does not.
    #[test]
    fn served_again_until_a_byte_it_read_changes() {
        let memo: BuffersMemo<u32, Option<u8>> = BuffersMemo::default();
        let ran = Cell::new(0);
        let memory = Memory::new();
        assert_eq!(memo.answer(7, &memory, |m| chase(m, &ran)), Some(0x10));
        assert_eq!(memo.answer(7, &memory, |m| chase(m, &ran)), Some(0x10));
        assert_eq!(ran.get(), 1, "the same bytes: served");
        // A byte it never read, though inside the span it asked was mapped.
        let memory = memory.with(0x80, 0);
        assert_eq!(memo.answer(7, &memory, |m| chase(m, &ran)), Some(0x10));
        assert_eq!(ran.get(), 1, "a byte it did not read: still served");
        // The byte its second read is chosen by.
        let memory = memory.with(0x10, 0x20);
        assert_eq!(memo.answer(7, &memory, |m| chase(m, &ran)), Some(0x20));
        assert_eq!(ran.get(), 2, "a byte it read: worked out again");
        // Another key is another question.
        assert_eq!(memo.answer(8, &memory, |m| chase(m, &ran)), Some(0x20));
        assert_eq!(ran.get(), 3);
    }

    /// The reads a run repeats are kept once, and touching ones are checked as one span.
    #[test]
    fn repeated_and_touching_reads_merge() {
        let memory = Memory::new();
        let recording = super::Recording::new(&memory);
        for _ in 0..1000 {
            let _ = recording.read(0x1000, 4);
        }
        let _ = recording.read(0x1004, 4);
        let _ = recording.read(0x1002, 4);
        let _ = recording.read(0x1040, 4);
        let reads = recording.read_set().expect("every span reads back");
        assert_eq!(reads.held, vec![(0x1000, 8), (0x1040, 4)]);
        assert!(reads.mapped.is_empty() && reads.unmapped.is_empty());
    }
}
