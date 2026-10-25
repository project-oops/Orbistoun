//! Guest memory a dispatch reaches by device address (D964).
//!
//! Some dispatches read memory whose place depends on what they compute - a ray traversal reads the
//! nodes its intersections name - so the host cannot work out each one's ranges before it runs, as
//! it does for every other dispatch (D746, D753, D758). Such a dispatch runs device-addressed
//! instead: every guest memory mapping is mapped whole into the device's address space, and the
//! translated module turns each guest address it forms into a device address through a table the
//! host keeps, by the 64-bit pointer the address names.
//!
//! The table is keyed by chunk: a guest address's top bits, the address shifted right by the chunk
//! shift the host chose, at most 64 KiB and never coarser than any mapping's alignment, so one chunk
//! lies in one mapping. A chunk's slot holds what to add to a guest address in it to reach the
//! device address, which is one value across a mapping, since a mapping is mapped contiguously. The
//! slots form an open-addressed hash table with a bounded probe, as a dispatch's global pages do
//! (D961), and the host grows it until every chunk lies within [`PROBES`] slots of its own.
//!
//! What a module tells it reaches by device address and what it reaches otherwise:
//!
//! - a global or flat access, its address formed per lane; a flat access first routes by aperture,
//!   its address's high half [`LOCAL_APERTURE_HIGH`] reaching the local data share and
//!   [`PRIVATE_APERTURE_HIGH`] the lane's private memory, as the hardware's apertures do;
//! - a buffer or scalar access whose descriptor or base the host could not resolve without running
//!   the program ([`resolved_only`]);
//! - a ray intersection's node.
//!
//! The buffers the host does resolve stay bound as before, so a format access still converts by
//! the format the host read from its descriptor.

use std::collections::{BTreeMap, BTreeSet};

use orbistoun_shader::{Decode, EncodingTable};
use orbistoun_spirv::Id;

use crate::draw_buffers::{BufferSource, DrawBuffers};
use crate::wavefront::{MOST_SAMPLED_BUFFERS, SAMPLED_BASES_PUSH_WORD};

/// Where a device-addressed dispatch's table sits in the push-constant block: seven words after the
/// bases of the buffers placed from a sample - the table's device address, low half then high, how
/// many home slots it has as a power of two, the chunk shift, the device address of its marks
/// ([`Table::mark_words`]), low half then high, and whether the dispatch records the chunks it
/// touches (D969), one or zero.
pub const DEVICE_TABLE_PUSH_WORD: u32 = SAMPLED_BASES_PUSH_WORD + 2 * MOST_SAMPLED_BUFFERS;

/// How many push-constant words [`DEVICE_TABLE_PUSH_WORD`] begins.
pub const DEVICE_TABLE_PUSH_WORDS: u32 = 7;

/// The high half of a flat address in the local data share's aperture: the low half is then a byte
/// of the workgroup's share. Grand Theft Auto V's ray traversal clamps a stack pointer's high half
/// between this and [`PRIVATE_APERTURE_HIGH`] and reads the entries it stored through the first
/// with `ds_read_b32` (guest-observed, the programs at 0x74005b854a00, 0x74005b857200 and
/// 0x74005b7e8600).
pub const LOCAL_APERTURE_HIGH: u32 = 0x8000_0000;

/// The high half of a flat address in the private (scratch) aperture: the low half is then the
/// lane's own byte of its private memory, as a scratch-segment access names it. The same programs
/// read the entries they stored through it with `scratch_load_dword` (guest-observed).
pub const PRIVATE_APERTURE_HIGH: u32 = 0x7000_0000;

/// The words before the table's first slot: a sink a lookup that finds nothing points a load at
/// instead, so the load needs no branch and reaches no address nothing maps. Sixteen bytes, so
/// every slot after it starts on sixteen.
pub const HEADER_WORDS: u32 = 4;

/// The words of one slot, sixteen bytes the module reads as one load: the chunk number's low half
/// and its high half, [`EMPTY`] in an unused slot, then what a guest address in the chunk adds to
/// reach its device address, low half then high.
///
/// The table is only read on the device, so it can lie in device-local memory; what a store marks
/// lies apart, in the written marks ([`Table::mark_words`]).
pub const SLOT_WORDS: u32 = 4;

/// The words before the first slot's written mark: a sink a store or atomic that lands nowhere
/// marks instead, so the mark needs no branch. A device store passes the host's write-watch by, so
/// it sets its chunk's mark, one word for each slot of the table at the slot's index, and the host
/// reads them back after the dispatch to learn which chunks it wrote, and clears them.
///
/// The touched marks follow the written ones, one word for each slot again: a recording dispatch's
/// every lookup that finds its chunk sets the chunk's, and the host reads them back to learn which
/// chunks the dispatch reached, the measure of what is worth a copy in device memory (D969).
pub const MARK_SINK_WORDS: u32 = 4;

/// How many slots from its own a chunk's lookup searches: consecutive slots, never wrapping, the
/// table holding `PROBES - 1` slots past its last home.
pub const PROBES: u32 = 4;

/// An unused slot's chunk high half: never a chunk's, whose high half is an address's bits above
/// 44 at most.
pub const EMPTY: u32 = u32::MAX;

/// The multiplier of [`chunk_slot`]'s hash: the golden ratio's fraction of 2^32.
pub const CHUNK_HASH: u32 = 0x9e37_79b1;

/// The coarsest chunk, 64 KiB, and the finest, 4 KiB: the host's page.
pub const MOST_CHUNK_SHIFT: u32 = 16;
/// See [`MOST_CHUNK_SHIFT`].
pub const LEAST_CHUNK_SHIFT: u32 = 12;

/// The most slots a table grows to, as a power of two: 2^28 slots of sixteen bytes, 4 GiB.
const MOST_BITS: u32 = 28;

/// What a model gives the device-address lookup it emits (D964): the 64-bit integer type, the
/// physical pointer types to one word and to one slot, and the table's device address, size, chunk
/// shift and written marks as the push block carries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceTable {
    /// The 64-bit unsigned integer type.
    pub u64_type: Id,
    /// A pointer to one 32-bit word through a device address.
    pub word_pointer: Id,
    /// Four 32-bit words, one slot.
    pub slot_type: Id,
    /// A pointer to one slot through a device address, aligned to sixteen bytes.
    pub slot_pointer: Id,
    /// The table's device address, a 64-bit value.
    pub address: Id,
    /// How many home slots it has, as a power of two.
    pub bits: Id,
    /// The chunk shift.
    pub shift: Id,
    /// The device address of the written marks, a 64-bit value.
    pub marks: Id,
    /// Whether the dispatch records the chunks it touches: one or zero.
    pub recording: Id,
}

/// The slot a chunk numbered `chunk` hashes to in a table of `1 << bits` slots: the halves of its
/// number mixed, by a multiplicative hash's top `bits` bits. The module forms the same.
#[must_use]
pub fn chunk_slot(chunk: u64, bits: u32) -> u32 {
    let mixed = (chunk as u32) ^ ((chunk >> 32) as u32);
    mixed.wrapping_mul(CHUNK_HASH) >> (32 - bits.clamp(1, 31))
}

/// One guest memory mapping as the device maps it: where it starts in guest memory, how many
/// bytes, and the device address its first byte has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapped {
    /// Its first guest address.
    pub guest: u64,
    /// Its length in bytes.
    pub len: u64,
    /// The device address of its first byte.
    pub device: u64,
}

/// Why no table could be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TableError {
    /// Two mappings share a byte.
    #[error("guest mappings at {first:#x} and {second:#x} overlap")]
    Overlap {
        /// The earlier mapping's start.
        first: u64,
        /// The later one's.
        second: u64,
    },
    /// A mapping is empty or not aligned to the host's page.
    #[error("a guest mapping at {guest:#x}+{len:#x} is empty or not page-aligned")]
    Misaligned {
        /// Its start.
        guest: u64,
        /// Its length.
        len: u64,
    },
    /// More chunks than the largest table holds.
    #[error("{chunks} chunks do not fit a device-address table")]
    TooLarge {
        /// How many there are.
        chunks: usize,
    },
}

/// A device-address table, as the host writes it into device-visible memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Its words: [`HEADER_WORDS`], then [`Self::slots`] slots of [`SLOT_WORDS`].
    pub words: Vec<u32>,
    /// How many home slots it has, as a power of two.
    pub bits: u32,
    /// The chunk shift: a chunk is `1 << shift` bytes.
    pub shift: u32,
}

impl Table {
    /// The table for `mapped`: the coarsest chunk every mapping's start and length keep, up to
    /// 64 KiB, and the fewest slots that place every chunk within [`PROBES`] of its own.
    ///
    /// # Errors
    ///
    /// Overlapping mappings, an empty or unaligned one, or more chunks than a table holds.
    pub fn build(mapped: &[Mapped]) -> Result<Self, TableError> {
        let mut sorted = mapped.to_vec();
        sorted.sort_by_key(|mapping| mapping.guest);
        let mut shift = MOST_CHUNK_SHIFT;
        for mapping in &sorted {
            let alignment = (mapping.guest | mapping.len).trailing_zeros();
            if mapping.len == 0 || alignment < LEAST_CHUNK_SHIFT {
                return Err(TableError::Misaligned {
                    guest: mapping.guest,
                    len: mapping.len,
                });
            }
            shift = shift.min(alignment);
        }
        for pair in sorted.windows(2) {
            if pair[0].guest.saturating_add(pair[0].len) > pair[1].guest {
                return Err(TableError::Overlap {
                    first: pair[0].guest,
                    second: pair[1].guest,
                });
            }
        }
        let chunks: Vec<(u64, u64)> = sorted
            .iter()
            .flat_map(|mapping| {
                let delta = mapping.device.wrapping_sub(mapping.guest);
                let first = mapping.guest >> shift;
                let end = (mapping.guest + mapping.len) >> shift;
                (first..end).map(move |chunk| (chunk, delta))
            })
            .collect();
        let mut bits = (chunks.len().max(1) * 2)
            .next_power_of_two()
            .trailing_zeros()
            .max(1);
        loop {
            if let Some(words) = place(&chunks, bits) {
                return Ok(Self { words, bits, shift });
            }
            bits += 1;
            if bits > MOST_BITS {
                return Err(TableError::TooLarge {
                    chunks: chunks.len(),
                });
            }
        }
    }

    /// How many slots it holds: its `1 << bits` homes and the [`PROBES`] less one past the last,
    /// which a lookup from the last home reads rather than wrapping.
    #[must_use]
    pub const fn slots(&self) -> u32 {
        (1 << self.bits) + PROBES - 1
    }

    /// How many words its marks take: [`MARK_SINK_WORDS`], then one written mark for each slot, then
    /// one touched mark for each slot.
    #[must_use]
    pub const fn mark_words(&self) -> usize {
        (MARK_SINK_WORDS + 2 * self.slots()) as usize
    }

    /// The push-constant words [`DEVICE_TABLE_PUSH_WORD`] begins, for a table at device address
    /// `address` whose marks are at device address `marks`, in a dispatch that records the chunks
    /// it touches where `recording`.
    #[must_use]
    pub const fn push_words(
        &self,
        address: u64,
        marks: u64,
        recording: bool,
    ) -> [u32; DEVICE_TABLE_PUSH_WORDS as usize] {
        #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
        [
            address as u32,
            (address >> 32) as u32,
            self.bits,
            self.shift,
            marks as u32,
            (marks >> 32) as u32,
            recording as u32,
        ]
    }

    /// The slot holding the chunk of guest address `guest`, where the table holds it.
    #[must_use]
    pub fn slot_of(&self, guest: u64) -> Option<u32> {
        let chunk = guest >> self.shift;
        let home = chunk_slot(chunk, self.bits);
        (home..home + PROBES).find(|&slot| {
            let at = (HEADER_WORDS + slot * SLOT_WORDS) as usize;
            self.words.get(at..at + 2).is_some_and(|entry| {
                entry[1] != EMPTY && (u64::from(entry[0]) | (u64::from(entry[1]) << 32)) == chunk
            })
        })
    }

    /// Points the chunk of guest address `guest` at a copy of it whose first byte is at device
    /// address `device` (D969): every guest address in the chunk then looks up in the copy. Whether
    /// the table holds the chunk.
    pub fn redirect(&mut self, guest: u64, device: u64) -> bool {
        let Some(slot) = self.slot_of(guest) else {
            return false;
        };
        let base = guest >> self.shift << self.shift;
        let delta = device.wrapping_sub(base);
        let at = (HEADER_WORDS + slot * SLOT_WORDS + 2) as usize;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the halves of a 64-bit word"
        )]
        self.words[at..at + 2].copy_from_slice(&[delta as u32, (delta >> 32) as u32]);
        true
    }

    /// The first guest address of each chunk whose touched mark in `marks` - this table's marks as
    /// the device left them ([`Self::mark_words`]) - is set, with every touched mark cleared.
    #[must_use]
    pub fn take_touched(&self, marks: &mut [u32]) -> Vec<u64> {
        let from = (MARK_SINK_WORDS + self.slots()) as usize;
        let mut touched = Vec::new();
        for (slot, mark) in (0u32..).zip(marks.iter_mut().skip(from)) {
            if std::mem::take(mark) == 0 {
                continue;
            }
            let at = (HEADER_WORDS + slot * SLOT_WORDS) as usize;
            let Some(entry) = self.words.get(at..at + 2) else {
                break;
            };
            if entry[1] != EMPTY {
                let chunk = u64::from(entry[0]) | (u64::from(entry[1]) << 32);
                touched.push(chunk << self.shift);
            }
        }
        touched.sort_unstable();
        touched
    }

    /// The device address of guest address `guest`, looked up as the module looks it up; `None`
    /// where no mapping holds it.
    #[must_use]
    pub fn lookup(&self, guest: u64) -> Option<u64> {
        let chunk = guest >> self.shift;
        let home = chunk_slot(chunk, self.bits);
        (0..PROBES).find_map(|probe| {
            let slot = home + probe;
            let at = (HEADER_WORDS + slot * SLOT_WORDS) as usize;
            let entry = self.words.get(at..at + SLOT_WORDS as usize)?;
            let held = u64::from(entry[0]) | (u64::from(entry[1]) << 32);
            let delta = u64::from(entry[2]) | (u64::from(entry[3]) << 32);
            (entry[1] != EMPTY && held == chunk).then(|| guest.wrapping_add(delta))
        })
    }

    /// The guest ranges, a chunk each, whose marks in `marks` - this table's written marks as the
    /// device left them ([`Self::mark_words`]) - say a store wrote them, with every mark cleared,
    /// the sink's too.
    #[must_use]
    pub fn take_written(&self, marks: &mut [u32]) -> Vec<(u64, u64)> {
        let chunk_bytes = 1u64 << self.shift;
        let sink = (MARK_SINK_WORDS as usize).min(marks.len());
        marks[..sink].fill(0);
        let mut written = Vec::new();
        let written_marks = marks[sink..].iter_mut().take(self.slots() as usize);
        for (slot, mark) in (0u32..).zip(written_marks) {
            if *mark == 0 {
                continue;
            }
            *mark = 0;
            let at = (HEADER_WORDS + slot * SLOT_WORDS) as usize;
            let Some(entry) = self.words.get(at..at + SLOT_WORDS as usize) else {
                break;
            };
            if entry[1] == EMPTY {
                continue;
            }
            let chunk = u64::from(entry[0]) | (u64::from(entry[1]) << 32);
            written.push((chunk << self.shift, chunk_bytes));
        }
        written.sort_unstable();
        written
    }
}

/// The table's words with `chunks` placed in `1 << bits` home slots and the [`PROBES`] less one
/// after, each within [`PROBES`] of its own; `None` where one does not fit.
fn place(chunks: &[(u64, u64)], bits: u32) -> Option<Vec<u32>> {
    let slots = (1usize << bits) + PROBES as usize - 1;
    let mut words = vec![0u32; HEADER_WORDS as usize + slots * SLOT_WORDS as usize];
    for slot in 0..slots {
        words[HEADER_WORDS as usize + slot * SLOT_WORDS as usize + 1] = EMPTY;
    }
    for &(chunk, delta) in chunks {
        let home = chunk_slot(chunk, bits);
        let free = (0..PROBES)
            .map(|probe| home + probe)
            .find(|&slot| words[(HEADER_WORDS + slot * SLOT_WORDS + 1) as usize] == EMPTY)?;
        let at = (HEADER_WORDS + free * SLOT_WORDS) as usize;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the halves of a 64-bit word"
        )]
        words[at..at + SLOT_WORDS as usize].copy_from_slice(&[
            chunk as u32,
            (chunk >> 32) as u32,
            delta as u32,
            (delta >> 32) as u32,
        ]);
    }
    Some(words)
}

/// The byte offsets of a program's global and flat accesses - the flat encoding's flat and global
/// segments, not its scratch one (D954) - which a device-addressed dispatch reaches by device
/// address and so leaves out of its traced buffers.
#[must_use]
pub fn flat_accesses(decode: &Decode, encodings: &EncodingTable) -> BTreeSet<u32> {
    decode
        .instructions
        .iter()
        .filter(|instruction| {
            instruction
                .encoding
                .and_then(|index| encodings.encodings().get(usize::from(index)))
                .is_some_and(|family| family.name == "FLAT")
                && (instruction.word >> 14) & 3 != 1
        })
        .map(|instruction| instruction.offset)
        .collect()
}

/// A device-addressed dispatch's buffers: those the host resolves from user data, constants and
/// loads at fixed places, renumbered in order. A buffer whose descriptor or base the program
/// computes (D753) or a global buffer (D758, D961) would need the program run on the host to place
/// it; its accesses reach guest memory by device address instead, and have no slot.
#[must_use]
pub fn resolved_only(buffers: DrawBuffers) -> DrawBuffers {
    let mut renumbered: BTreeMap<u32, u32> = BTreeMap::new();
    let mut sources = Vec::new();
    for (slot, source) in (0u32..).zip(buffers.sources) {
        if matches!(source, BufferSource::Computed(_) | BufferSource::Global(_)) {
            continue;
        }
        renumbered.insert(slot, u32::try_from(sources.len()).unwrap_or(u32::MAX));
        sources.push(source);
    }
    let kept = |slots: BTreeSet<u32>| -> BTreeSet<u32> {
        slots
            .into_iter()
            .filter_map(|slot| renumbered.get(&slot).copied())
            .collect()
    };
    DrawBuffers {
        sources,
        served: buffers
            .served
            .into_iter()
            .filter_map(|(offset, slot)| Some((offset, *renumbered.get(&slot)?)))
            .collect(),
        stored_only: kept(buffers.stored_only),
        stored: kept(buffers.stored),
        untraced_by: buffers.untraced_by,
        // A table's slots are computed ones, and its records would lie in the global buffer: both
        // are gone, and what they reached is reached by device address.
        tables: BTreeMap::new(),
        several: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_buffers::{Computed, ComputedAccess, DescriptorReads, DescriptorWord};

    const GUEST: u64 = 0x7400_5b80_0000;

    /// Every byte of every mapping looks up to its own device address, and a byte between two
    /// mappings, or past the last, finds nothing.
    #[test]
    fn a_table_finds_every_mapped_address_and_nothing_else() {
        let mapped = [
            Mapped {
                guest: GUEST,
                len: 0x30_0000,
                device: 0x1_2340_0000,
            },
            Mapped {
                guest: GUEST + 0x40_0000,
                len: 0x1_0000,
                device: 0x9000_0000,
            },
            Mapped {
                guest: 0x4000_0000_0000,
                len: 0x2000_0000,
                device: 0x7_0000_0000,
            },
        ];
        let table = Table::build(&mapped).expect("a table");
        assert_eq!(table.shift, MOST_CHUNK_SHIFT, "every mapping keeps 64 KiB");
        for mapping in &mapped {
            for offset in [0, 4, 0xfffc, mapping.len / 2, mapping.len - 4] {
                assert_eq!(
                    table.lookup(mapping.guest + offset),
                    Some(mapping.device + offset),
                    "{:#x}",
                    mapping.guest + offset
                );
            }
        }
        assert_eq!(
            table.lookup(GUEST + 0x30_0000),
            None,
            "between two mappings"
        );
        assert_eq!(table.lookup(GUEST + 0x41_0000), None, "past one");
        assert_eq!(table.lookup(0x10_0000), None, "nothing maps it");
    }

    /// A mapping kept only to 16 KiB makes every chunk 16 KiB, so no chunk straddles it and its
    /// neighbour, whose device addresses differ.
    #[test]
    fn a_finer_mapping_makes_finer_chunks() {
        let mapped = [
            Mapped {
                guest: GUEST,
                len: 0x4000,
                device: 0x10_0000,
            },
            Mapped {
                guest: GUEST + 0x4000,
                len: 0xc000,
                device: 0x80_0000,
            },
        ];
        let table = Table::build(&mapped).expect("a table");
        assert_eq!(table.shift, 14);
        assert_eq!(table.lookup(GUEST + 0x3ffc), Some(0x10_3ffc));
        assert_eq!(table.lookup(GUEST + 0x4000), Some(0x80_0000));
    }

    /// Overlapping mappings, and one not on a page, are refused rather than shadowing each other.
    #[test]
    fn overlapping_or_unaligned_mappings_are_refused() {
        let overlapping = [
            Mapped {
                guest: GUEST,
                len: 0x2_0000,
                device: 0,
            },
            Mapped {
                guest: GUEST + 0x1_0000,
                len: 0x1_0000,
                device: 0x100_0000,
            },
        ];
        assert!(matches!(
            Table::build(&overlapping),
            Err(TableError::Overlap { .. })
        ));
        let unaligned = [Mapped {
            guest: GUEST + 0x800,
            len: 0x1000,
            device: 0,
        }];
        assert!(matches!(
            Table::build(&unaligned),
            Err(TableError::Misaligned { .. })
        ));
    }

    /// Many chunks still each lie within the probe of their own slot: the table grows until they do.
    #[test]
    fn every_chunk_lies_within_the_probe() {
        let mapped: Vec<Mapped> = (0..300u64)
            .map(|i| Mapped {
                guest: GUEST + i * 0x10_0000,
                len: 0x8_0000,
                device: 0x100_0000_0000 + i * 0x20_0000,
            })
            .collect();
        let table = Table::build(&mapped).expect("a table");
        for mapping in &mapped {
            assert_eq!(
                table.lookup(mapping.guest + 0x7_fffc),
                Some(mapping.device + 0x7_fffc)
            );
        }
        let words = [table.bits, table.shift];
        assert_eq!(table.push_words(0x1234_5678_9abc, 0, false)[2..4], words);
    }

    /// A slot a store marked comes back as its chunk's range, once: the mark is cleared. A mark in
    /// the sink, where a store that landed nowhere points, names nothing.
    #[test]
    fn marked_slots_come_back_once_as_their_chunks() {
        let table = Table::build(&[Mapped {
            guest: GUEST,
            len: 0x4_0000,
            device: 0x10_0000,
        }])
        .expect("a table");
        let mut marks = vec![0u32; table.mark_words()];
        let chunk = (GUEST + 0x2_0000) >> table.shift;
        let slot = (0..table.slots())
            .find(|slot| {
                let at = (HEADER_WORDS + slot * SLOT_WORDS) as usize;
                table.words[at] == chunk as u32 && table.words[at + 1] != EMPTY
            })
            .expect("its slot");
        marks[(MARK_SINK_WORDS + slot) as usize] = 1;
        marks[0] = 1;
        assert_eq!(
            table.take_written(&mut marks),
            vec![(GUEST + 0x2_0000, 0x1_0000)]
        );
        assert!(table.take_written(&mut marks).is_empty(), "cleared");
    }

    /// A slot a recording dispatch touched comes back as its chunk's base, once; the written marks
    /// before them are left alone (D969).
    #[test]
    fn touched_slots_come_back_once_as_their_chunks() {
        let table = Table::build(&[Mapped {
            guest: GUEST,
            len: 0x4_0000,
            device: 0x10_0000,
        }])
        .expect("a table");
        let mut marks = vec![0u32; table.mark_words()];
        let chunk = (GUEST + 0x3_0000) >> table.shift;
        let slot = table.slot_of(GUEST + 0x3_0000).expect("its slot");
        assert_eq!(
            table.words[(HEADER_WORDS + slot * SLOT_WORDS) as usize],
            chunk as u32
        );
        marks[(MARK_SINK_WORDS + table.slots() + slot) as usize] = 1;
        marks[(MARK_SINK_WORDS + slot) as usize] = 1;
        assert_eq!(table.take_touched(&mut marks), vec![GUEST + 0x3_0000]);
        assert!(table.take_touched(&mut marks).is_empty(), "cleared");
        assert_eq!(
            table.take_written(&mut marks),
            vec![(GUEST + 0x3_0000, 0x1_0000)],
            "the written mark kept"
        );
    }

    /// A chunk redirected to a copy elsewhere looks up there, every byte of it, and its neighbours
    /// do not move; a chunk the table does not hold is not redirected.
    #[test]
    fn a_redirected_chunk_looks_up_at_its_copy() {
        let mut table = Table::build(&[Mapped {
            guest: GUEST,
            len: 0x4_0000,
            device: 0x10_0000,
        }])
        .expect("a table");
        assert!(table.redirect(GUEST + 0x1_0000, 0x9_0000_0000));
        assert_eq!(table.lookup(GUEST + 0x1_0004), Some(0x9_0000_0004));
        assert_eq!(table.lookup(GUEST + 0x1_fffc), Some(0x9_0000_fffc));
        assert_eq!(table.lookup(GUEST + 0x2_0000), Some(0x12_0000));
        assert!(!table.redirect(GUEST + 0x8_0000, 0x9_0000_0000));
    }

    /// A lookup's slots are consecutive and never wrap: the table holds [`PROBES`] less one slots
    /// past its last home, so every probe lies in its words, and a slot starts on 16 bytes, so the
    /// module reads it as one four-word load.
    #[test]
    fn a_lookup_reads_consecutive_slots_on_sixteen_bytes() {
        let table = Table::build(&[Mapped {
            guest: GUEST,
            len: 0x40_0000,
            device: 0x10_0000,
        }])
        .expect("a table");
        assert_eq!(table.slots(), (1 << table.bits) + PROBES - 1);
        assert_eq!(
            table.words.len(),
            (HEADER_WORDS + table.slots() * SLOT_WORDS) as usize
        );
        assert_eq!((HEADER_WORDS * 4) % 16, 0);
        assert_eq!((SLOT_WORDS * 4) % 16, 0);
        assert_eq!(
            table.mark_words(),
            (MARK_SINK_WORDS + 2 * table.slots()) as usize
        );
        assert_eq!(
            table.push_words(0x1_2345_6780, 0xabcd_ef00_0000, true),
            [
                0x2345_6780,
                1,
                table.bits,
                table.shift,
                0xef00_0000,
                0xabcd,
                1
            ]
        );
    }

    /// No mapping is a table every lookup misses.
    #[test]
    fn no_mapping_is_a_table_that_finds_nothing() {
        let table = Table::build(&[]).expect("a table");
        assert_eq!(table.lookup(GUEST), None);
    }

    /// A computed buffer and the global buffer are dropped from a device-addressed dispatch's
    /// buffers and the rest renumbered, with the accesses through them and their store marks.
    #[test]
    fn only_resolved_buffers_keep_a_slot() {
        let resolved = BufferSource::Descriptor {
            words: [DescriptorWord::UserData(0); 4],
            reads: DescriptorReads::default(),
        };
        let computed = BufferSource::Computed(Box::new(Computed {
            prefix: Vec::new(),
            at: 0x10,
            through: ComputedAccess::Pointer { base: 4, end: 4 },
            soffset: 125,
            first_register: 0,
            user_data: 2,
            program_address: None,
        }));
        let buffers = DrawBuffers {
            sources: vec![computed, resolved.clone()],
            served: [(0x10, 0), (0x20, 1)].into_iter().collect(),
            stored_only: BTreeSet::new(),
            stored: [0, 1].into_iter().collect(),
            untraced_by: None,
            ..DrawBuffers::default()
        };
        let kept = resolved_only(buffers);
        assert_eq!(kept.sources, vec![resolved]);
        assert_eq!(kept.served, [(0x20, 0)].into_iter().collect());
        assert_eq!(kept.stored, [0].into_iter().collect());
    }

    /// The flat and global segments are flat accesses; the scratch segment is not.
    #[test]
    fn flat_accesses_leave_out_the_scratch_segment() {
        use orbistoun_shader::{OperandTable, decode};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let (family, opcode) = encodings
            .find_by_name("global_load_dword")
            .expect("global_load_dword");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        let head = (encoding.value | (opcode << encoding.opcode.shift)) & !(3 << 14);
        // global_load_dword v1, v[2:3], off in the flat, scratch and global segments; s_endpgm.
        let second = 0x017d_0002;
        let words = [
            head,
            second,
            head | 1 << 14,
            0x017d_0002,
            head | 2 << 14,
            second,
            0xbf81_0000,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        assert_eq!(
            flat_accesses(&decoded, &encodings),
            [0, 16].into_iter().collect()
        );
    }
}
