//! Descriptor ranges resolved once a preparation for the words they are formed from.
//!
//! A frame's draws mostly bind the same buffers through the same descriptor words, and resolving one
//! reads its table words through the settling reads a preparation makes, each a handful of locks and
//! list walks. A descriptor's range is a function of its user-data words, the table words it loads
//! and, where its accesses index by the draw's ids, those ids; guest memory does not change while a
//! submission is prepared, so the range found for one draw is the range for every later draw that
//! forms the descriptor from the same words. A computed descriptor's range is likewise a function
//! of the registers its replay starts from (D753).

use std::collections::HashMap;

use orbistoun_translate::draw_buffers::{
    BufferSource, ComputedAccess, DescriptorWord, RecordIndex,
};

use super::{DrawIds, Refusal, USER_DATA_WORDS, resolve_draw_range, table_address};
use crate::pipeline::GuestMemory;

/// What a descriptor source's range depends on for one draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Inputs {
    /// The source, by its place in the module's list, which outlives the preparation.
    source: usize,
    /// Each descriptor word's input: a user-data word's value, a loaded word's address, nothing for
    /// a constant. The source fixes which each is.
    words: [u64; 4],
    /// The draw's ids and the input of the word added to them, where the accesses index by them.
    index: Option<(DrawIds, u64)>,
}

/// What a computed source's range depends on for one draw (D753): the stage's user-data words and
/// the system registers its replay starts from, and the draw's ids where its accesses index by them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ComputedInputs {
    /// The source, as [`Inputs::source`].
    source: usize,
    user_data: [u32; USER_DATA_WORDS],
    system: Vec<(u32, u32)>,
    ids: Option<DrawIds>,
}

/// The ranges one preparation has resolved, by what they depend on.
#[derive(Debug, Default)]
pub(super) struct Resolved {
    held: HashMap<Inputs, Result<(u64, u64), Refusal>>,
    /// A computed source's answers. Its own kept answers hold across preparations, but each is
    /// found by hashing the program and checked by reading again every word the replay read; asked
    /// again within a preparation, where memory stands still, neither is needed.
    computed: HashMap<ComputedInputs, Result<(u64, u64), Refusal>>,
}

impl Resolved {
    /// [`resolve_draw_range`], answered from an earlier draw of this preparation that formed a
    /// descriptor source from the same words, or ran a computed source's replay from the same
    /// registers. A global source is resolved every time: it depends on the draw's vertex count.
    pub(super) fn range(
        &mut self,
        source: &BufferSource,
        draw: (&[u32; USER_DATA_WORDS], u32, &[(u32, u32)]),
        ids: Option<DrawIds>,
        memory: &impl GuestMemory,
    ) -> Result<(u64, u64), Refusal> {
        if let Some(inputs) = computed_inputs_of(source, draw, ids) {
            if let Some(answer) = self.computed.get(&inputs) {
                return answer.clone();
            }
            let answer = resolve_draw_range(source, draw, ids, memory);
            self.computed.insert(inputs, answer.clone());
            return answer;
        }
        let Some(inputs) = inputs_of(source, draw.0, ids) else {
            return resolve_draw_range(source, draw, ids, memory);
        };
        self.held
            .entry(inputs)
            .or_insert_with(|| resolve_draw_range(source, draw, ids, memory))
            .clone()
    }
}

/// What a computed source's range depends on for a draw, or `None` for any other source. The ids
/// count only where the access is through a descriptor whose reads index by them.
fn computed_inputs_of(
    source: &BufferSource,
    (user_data, _, system): (&[u32; USER_DATA_WORDS], u32, &[(u32, u32)]),
    ids: Option<DrawIds>,
) -> Option<ComputedInputs> {
    let BufferSource::Computed(computed) = source else {
        return None;
    };
    let indexed = matches!(
        computed.through,
        ComputedAccess::Descriptor { reads, .. } if reads.index.is_some()
    );
    Some(ComputedInputs {
        source: std::ptr::from_ref(source).addr(),
        user_data: *user_data,
        system: system.to_vec(),
        ids: ids.filter(|_| indexed),
    })
}

/// What a descriptor source's range depends on for a draw with these user-data words and ids, or
/// `None` for any other source.
fn inputs_of(
    source: &BufferSource,
    user_data: &[u32; USER_DATA_WORDS],
    ids: Option<DrawIds>,
) -> Option<Inputs> {
    let BufferSource::Descriptor { words, reads } = source else {
        return None;
    };
    let input = |word: DescriptorWord| match word {
        // A word the stage does not have refuses the draw; no value a word holds is this.
        DescriptorWord::UserData(index) => usize::try_from(index)
            .ok()
            .and_then(|index| user_data.get(index))
            .map_or(u64::MAX, |&value| u64::from(value)),
        DescriptorWord::Constant(_) => 0,
        DescriptorWord::Loaded { table, offset } => {
            table_address(table, user_data).wrapping_add(u64::from(offset))
        }
    };
    let index = ids.zip(reads.index).map(|(ids, index)| {
        let (RecordIndex::Vertex(plus) | RecordIndex::Instance(plus)) = index;
        (ids, plus.map_or(0, input))
    });
    Some(Inputs {
        source: std::ptr::from_ref(source).addr(),
        words: words.map(input),
        index,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use orbistoun_translate::draw_buffers::{BufferSource, DescriptorReads, DescriptorWord};
    use orbistoun_translate::wavefront::{TableBase, TableWord};

    use super::{Resolved, USER_DATA_WORDS, resolve_draw_range};
    use crate::pipeline::GuestMemory;

    /// Guest memory at one place, counting the reads made of it.
    struct Counted {
        base: u64,
        bytes: Vec<u8>,
        reads: Cell<usize>,
    }

    impl GuestMemory for Counted {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            self.reads.set(self.reads.get() + 1);
            let at = usize::try_from(address.checked_sub(self.base)?).ok()?;
            self.bytes.get(at..at.checked_add(length)?)
        }
    }

    /// Two descriptor tables, each a buffer of 0x40 bytes at its own base.
    fn tables() -> Counted {
        let descriptor = |base: u32| [base, 0, 0x40, 0];
        let words: Vec<u32> = descriptor(0x1_0000)
            .into_iter()
            .chain(descriptor(0x2_0000))
            .collect();
        Counted {
            base: 0x8000,
            bytes: words.iter().flat_map(|word| word.to_le_bytes()).collect(),
            reads: Cell::new(0),
        }
    }

    /// A descriptor loaded whole from the table user-data words 0 and 1 point at.
    fn loaded() -> BufferSource {
        let table = TableBase {
            low: TableWord::UserData(0),
            high: TableWord::UserData(1),
        };
        BufferSource::Descriptor {
            words: [0, 4, 8, 12].map(|offset| DescriptorWord::Loaded { table, offset }),
            reads: DescriptorReads {
                scalar_reach: 0x40,
                ..DescriptorReads::default()
            },
        }
    }

    /// A later draw that forms the descriptor from the same words is answered without reading its
    /// table again, with the range an uncached resolve finds; one whose words point elsewhere reads
    /// its own table.
    #[test]
    fn a_descriptor_is_resolved_once_for_the_same_words() {
        let memory = tables();
        let source = loaded();
        let mut first = [0u32; USER_DATA_WORDS];
        first[0] = 0x8000;
        let mut other_draw = first;
        other_draw[5] = 0x1234;
        let mut second = first;
        second[0] = 0x8010;

        let mut resolved = Resolved::default();
        let range = resolved.range(&source, (&first, 3, &[]), None, &memory);
        let after_first = memory.reads.get();
        assert!(after_first > 0, "the first draw reads its table");
        assert_eq!(
            range,
            resolve_draw_range(&source, (&first, 3, &[]), None, &memory)
        );
        let uncached = memory.reads.get();

        let again = resolved.range(&source, (&other_draw, 7, &[]), None, &memory);
        assert_eq!(again, range, "a word it does not read changes nothing");
        assert_eq!(memory.reads.get(), uncached, "and nothing is read for it");

        let elsewhere = resolved.range(&source, (&second, 3, &[]), None, &memory);
        assert!(memory.reads.get() > uncached, "another table is read");
        assert_eq!(
            elsewhere,
            resolve_draw_range(&source, (&second, 3, &[]), None, &memory)
        );
        assert_ne!(elsewhere, range);
    }

    /// A computed descriptor asked again within a preparation from the same registers is answered
    /// without reading guest memory - not even to check a kept answer still holds - with what the
    /// replay found; from other registers it is replayed.
    #[test]
    fn a_computed_descriptor_is_replayed_once_for_the_same_registers() {
        use orbistoun_translate::draw_buffers::{Computed, ComputedAccess};
        // A descriptor at 0x9000: a buffer at 0x2000 of four sixteen-byte records.
        let table: Vec<u8> = [0x2000u32, 16 << 16, 4, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let memory = Counted {
            base: 0x9000,
            bytes: table,
            reads: Cell::new(0),
        };
        let source = BufferSource::Computed(Box::new(Computed {
            // s_load_dwordx4 s[0:3], s[4:5], 0x0; s_waitcnt lgkmcnt(0)
            prefix: vec![0xf408_0002, 0xfa00_0000, 0xbf8c_c07f],
            at: 12,
            through: ComputedAccess::Descriptor {
                first: 0,
                reads: DescriptorReads {
                    vector: true,
                    vector_reach: Some(16),
                    indexed: true,
                    ..DescriptorReads::default()
                },
            },
            soffset: 128,
            first_register: 4,
            user_data: 2,
            program_address: None,
        }));
        let mut user_data = [0u32; USER_DATA_WORDS];
        user_data[0] = 0x9000;

        let mut resolved = Resolved::default();
        let range = resolved.range(&source, (&user_data, 3, &[]), None, &memory);
        assert_eq!(range, Ok((0x2000, 64)), "{range:?}");
        let after_first = memory.reads.get();
        assert!(after_first > 0, "the replay reads its table");
        let again = resolved.range(&source, (&user_data, 3, &[]), None, &memory);
        assert_eq!(again, range);
        assert_eq!(memory.reads.get(), after_first, "nothing is read for it");

        // Outside the preparation, the kept answer is checked against memory.
        assert_eq!(
            resolve_draw_range(&source, (&user_data, 3, &[]), None, &memory),
            range
        );
        let checked = memory.reads.get();
        assert!(checked > after_first);

        user_data[0] = 0xA000;
        let elsewhere = resolved.range(&source, (&user_data, 3, &[]), None, &memory);
        assert!(elsewhere.is_err(), "no table there: {elsewhere:?}");
        assert!(memory.reads.get() > checked, "other registers are replayed");
    }
}
