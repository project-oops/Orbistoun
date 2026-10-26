//! The buffers a draw's shader reads through, traced to where a draw finds them (D733).
//!
//! A shader names a buffer by a four-word descriptor in scalar registers, or reads straight from a
//! 64-bit base with a scalar load. Each is traced back through the program to what the draw
//! supplies - its user-data words, constants, and words a scalar load read from a table at a base
//! the draw supplies - so a caller can resolve it per draw and bind the guest range it names. What
//! cannot be traced is left to the window.

use std::collections::BTreeMap;

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};
use serde::{Deserialize, Serialize};

use crate::wavefront::{TableBase, TableWord};

/// One word of a buffer descriptor, as the program formed it before reading through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DescriptorWord {
    /// The stage's user-data word at this index.
    UserData(u32),
    /// This value.
    Constant(u32),
    /// The word a scalar load read at `offset` bytes past `table`.
    Loaded {
        /// Where the table is.
        table: TableBase,
        /// Byte offset of the word in it.
        offset: u32,
    },
}

/// Where one of a module's buffers comes from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BufferSource {
    /// A buffer resource constant: its four words, and how the program reads through it. The range
    /// is the one the descriptor's bounds rules admit for those reads.
    Descriptor {
        /// The descriptor's words, in register order.
        words: [DescriptorWord; 4],
        /// What the accesses through it reach.
        reads: DescriptorReads,
    },
    /// A scalar load's base, and how many bytes past it the program's loads through it reach.
    Pointer {
        /// The base address.
        base: TableBase,
        /// One past the last byte any load through the base reads.
        extent: u32,
    },
    /// A descriptor, or a scalar offset, the program computes from more than user data,
    /// constants and loads at fixed places: found per draw by running the program up to the
    /// access (D753).
    Computed(Box<Computed>),
    /// A primitive shader's global loads, all through one buffer: the range their lanes' addresses
    /// span, found per draw by running the program lane by lane (D758).
    Global(Box<GlobalLoads>),
}

/// A primitive shader's global loads with no scalar base (D758), and a dispatch's global stores too
/// (D961): the program through the last of them, and where each is and how far past its address it
/// reads or writes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GlobalLoads {
    /// The program's words through the last access.
    pub prefix: Vec<u32>,
    /// Each access's byte offset in the program, and how many bytes it reads or writes from its
    /// address.
    pub loads: Vec<(u32, u32)>,
    /// The scalar register the stage's first user-data word lands in.
    pub first_register: u32,
    /// How many user-data words the stage has.
    pub user_data: u32,
    /// How many lanes the program's wave has, which only the stream's registers say: set by
    /// whoever knows them, sixty-four until then.
    pub width: u32,
}

/// An access whose descriptor or base the program computes (D753): the program before it, and the
/// registers that hold what it reads through and its scalar offset when it is reached.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Computed {
    /// The program's words before the access.
    pub prefix: Vec<u32>,
    /// The access's byte offset in the program.
    pub at: u32,
    /// What the access reads through.
    pub through: ComputedAccess,
    /// The scalar offset field's code: a register, `null`, or an inline constant.
    pub soffset: u8,
    /// The scalar register the stage's first user-data word lands in.
    pub first_register: u32,
    /// How many user-data words the stage has; the registers past them are unknown at entry.
    pub user_data: u32,
    /// Where the program was loaded, for an `s_getpc_b64` in the prefix (D842).
    #[serde(default)]
    pub program_address: Option<u64>,
}

/// What a computed access reads through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ComputedAccess {
    /// A buffer descriptor in four scalar registers from `first`, reaching what `reads` says from
    /// its base past the scalar offset.
    Descriptor {
        /// The descriptor's first register.
        first: u16,
        /// What the access reaches.
        reads: DescriptorReads,
    },
    /// A scalar load's 64-bit base in the pair from `base`, reaching `end` bytes past the base and
    /// the scalar offset.
    Pointer {
        /// The base's low register.
        base: u16,
        /// One past the last byte the load reads.
        end: u32,
    },
}

/// How a program's accesses through one descriptor reach into its buffer: what bounds the bytes a
/// draw must bind for them, beside the descriptor's own fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DescriptorReads {
    /// One past the last byte any scalar buffer load through it reads, from the buffer's start;
    /// zero when none does.
    pub scalar_reach: u32,
    /// Whether a vector access reads through it, so its out-of-bounds mode applies.
    pub vector: bool,
    /// One past the last byte any vector access reads from its record's start - its immediate
    /// offset and its width - or [`None`] when one adds a register offset, which nothing bounds
    /// here.
    pub vector_reach: Option<u32>,
    /// Whether a vector access indexes records.
    pub indexed: bool,
    /// What every indexed access indexes by, where the program shows it and they agree (D879):
    /// the draw then reaches only the records its ids name. [`None`] where any may index any
    /// record.
    #[serde(default)]
    pub index: Option<RecordIndex>,
}

/// What a vector access's record index is, as the program forms it (D879): a primitive shader's
/// vertex or instance id as the module seeds it, with the word added to it, if one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RecordIndex {
    /// The vertex id (`v5`), plus the word.
    Vertex(Option<DescriptorWord>),
    /// The instance id (`v8`, D863), plus the word.
    Instance(Option<DescriptorWord>),
}

impl DescriptorReads {
    /// These reads together with `other`'s.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        Self {
            scalar_reach: self.scalar_reach.max(other.scalar_reach),
            vector: self.vector || other.vector,
            vector_reach: self
                .vector_reach
                .zip(other.vector_reach)
                .map(|(a, b)| a.max(b)),
            indexed: self.indexed || other.indexed,
            // Only indexed accesses say what they index by, and all of them must agree.
            index: match (self.indexed, other.indexed) {
                (false, _) => other.index,
                (_, false) => self.index,
                _ => self.index.filter(|_| self.index == other.index),
            },
        }
    }
}

impl Default for DescriptorReads {
    fn default() -> Self {
        Self {
            scalar_reach: 0,
            vector: false,
            vector_reach: Some(0),
            indexed: false,
            index: None,
        }
    }
}

/// A buffer descriptor table a dispatch selects a record of at run time (D966): a scalar buffer
/// load of four words through the table's descriptor at a scalar offset the program computes, the
/// words then read through as a descriptor. Every record the table holds is bound, and the module
/// reads through whichever record the program selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SelectedTable {
    /// The table's own descriptor, as the program traced it.
    pub words: [DescriptorWord; 4],
    /// Where a record's descriptor starts in its record: the selecting load's immediate offset.
    pub element: u32,
}

/// An image access whose eight descriptor registers hold a record a dispatch selected from a table
/// at run time (D983): an `s_buffer_load_dwordx8` through the table's traced descriptor at an offset
/// register the program computes. The host binds every image the table's records name, and the
/// module finds the one the registers hold as it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SelectedImage {
    /// The table, and where a record's image descriptor starts in its record.
    pub table: SelectedTable,
    /// The sampler descriptor's four words, where the program formed them from user data,
    /// constants and loads at fixed places; `None` for a fetch, and for a sampler it computes.
    pub sampler: Option<[DescriptorWord; 4]>,
    /// Whether the access reads through a sampler: a sample or a gather, not a fetch or a size
    /// query. One whose sampler is not traced reads through a sampler the host cannot know.
    pub sampled: bool,
}

/// A module's buffers and the accesses that read through each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrawBuffers {
    /// Each buffer, by slot, in the order the program first reaches it.
    pub sources: Vec<BufferSource>,
    /// The slot each traced access reads through, by the access's byte offset in the program.
    pub served: BTreeMap<u32, u32>,
    /// The slots every access to which is a store (D809): a dispatch's buffer the program writes
    /// and never reads, whose words before the run nothing depends on but the ones it leaves.
    pub stored_only: std::collections::BTreeSet<u32>,
    /// The slots any access to which is a store: a dispatch's buffers it writes. Two read-only
    /// copies of one byte agree, so only an overlap with one of these is refused (D746).
    pub stored: std::collections::BTreeSet<u32>,
    /// The offset and name of the first store that kept the program from being traced at all: one
    /// a snapshot bound for a draw would not see, or a dispatch's that is not a buffer store.
    pub untraced_by: Option<(u32, String)>,
    /// A dispatch's computed slots whose descriptor is a record of a table it selects from at run
    /// time, with the table (D966). Where the host cannot find the descriptor before the program
    /// runs, every record of the table is bound, in the dispatch's global buffer of pages.
    pub tables: BTreeMap<u32, SelectedTable>,
    /// Those of [`Self::tables`]' slots that accesses at more than one place share, whose records
    /// may differ between them (D966): a run up to the first does not find the others', so the
    /// table is always bound whole.
    pub several: std::collections::BTreeSet<u32>,
    /// A dispatch's image accesses, by byte offset, whose descriptor is a record selected from a
    /// table at run time (D983).
    pub image_tables: BTreeMap<u32, SelectedImage>,
}

/// Traces the buffers a program reads through, for a stage whose `count` user-data words land in
/// the scalar registers from `first_register`.
///
/// Nothing is traced in a program that writes memory, which a snapshot bound for the draw would
/// not see - unless `stores` (a dispatch, whose buffers are written back, D746) and every write is
/// a buffer store through a descriptor, which is traced like a load, or a store to an image the
/// dispatch binds whole and writes back the same way. With `globals` (a primitive
/// shader, D758), its global loads with no scalar base share one buffer after the others, and an
/// indexed access by its vertex or instance id says so (D879). A dispatch's global loads share one
/// buffer the same way (D883).
#[must_use]
pub fn trace(
    decode: &Decode,
    encodings: &EncodingTable,
    registers: (u32, u32),
    (stores, globals): (bool, bool),
) -> DrawBuffers {
    trace_with(decode, encodings, registers, (stores, globals, stores))
}

/// [`trace`] with whether the program is a dispatch's apart from whether its stores are traced: a
/// pixel shader's stores are written back after its draw, as a dispatch's are, while its global
/// loads stay a draw's.
#[must_use]
pub fn trace_with(
    decode: &Decode,
    encodings: &EncodingTable,
    registers: (u32, u32),
    flags: (bool, bool, bool),
) -> DrawBuffers {
    trace_skipping(
        decode,
        encodings,
        (registers, flags),
        &std::collections::BTreeSet::new(),
    )
}

/// [`trace_with`] leaving out the accesses at the byte offsets `skipped` names: a dispatch's scratch
/// accesses, which reach the wave's own private memory and no buffer (D954).
#[must_use]
pub fn trace_skipping(
    decode: &Decode,
    encodings: &EncodingTable,
    ((first_register, count), (stores, globals, dispatch)): ((u32, u32), (bool, bool, bool)),
    skipped: &std::collections::BTreeSet<u32>,
) -> DrawBuffers {
    let named = named_instructions(decode, encodings);
    let untraced_by = untraced_by(decode, &named, (stores, dispatch), skipped);
    if untraced_by.is_some() {
        return DrawBuffers {
            untraced_by,
            ..DrawBuffers::default()
        };
    }
    let Ok(blocks) = crate::blocks::split(decode, |instruction| {
        let family = encodings
            .encodings()
            .get(usize::from(instruction.encoding?))?;
        Some(family.name.clone())
    }) else {
        return DrawBuffers::default();
    };
    let entry = entry_state(first_register, count);
    let reached = block_entries(decode, &named, &blocks, entry);
    let indices = if globals {
        lane_entries(decode, &named, &blocks, &reached)
    } else {
        vec![None; blocks.len()]
    };
    // Which registers hold a record a dispatch selected from a descriptor table at run time (D966).
    let selections = selection_entries(decode, &named, (&blocks, &reached), dispatch);

    // Every access, with the state it reads, in program order: the bases' reach is known only once
    // every load through them has been seen.
    let mut accesses: Vec<(u32, Access)> = Vec::new();
    // A dispatch's global loads are traced as a primitive shader's are (D883).
    let mut stores: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut atomics: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    // Each computed access that reads what an earlier one in its block read, with nothing between
    // them writing it: that earlier access, whose buffer it shares (D952).
    let mut shared = BTreeMap::new();
    for (((block, state), lanes), selected) in
        blocks.iter().zip(reached).zip(indices).zip(selections)
    {
        let Some(mut held) = state else {
            continue;
        };
        let mut lanes = lanes.unwrap_or_default();
        let mut selected = selected.unwrap_or_default();
        let mut sharing = ComputedSharing::new();
        for (instruction, named) in decode.instructions[block.first..block.end]
            .iter()
            .zip(&named[block.first..block.end])
        {
            if skipped.contains(&instruction.offset) {
                // A scratch access writes vector registers alone, which nothing here traces.
            } else if let Some(&(_, name)) = named.as_ref()
                && let Some(access) = access(instruction, name, &held, dispatch)
            {
                if bound_buffer_store(name) {
                    stores.insert(instruction.offset);
                }
                if name.starts_with("buffer_atomic_") {
                    atomics.insert(instruction.offset);
                }
                if let Access::Computed {
                    through, soffset, ..
                } = access
                    && let Some(first) = sharing.reads(instruction.offset, through, soffset)
                {
                    shared.insert(instruction.offset, first);
                }
                let access = indexed_by(selected_by(access, name, &selected), instruction, &lanes);
                accesses.push((instruction.offset, access));
            } else if let Some(&(_, name)) = named.as_ref()
                && (globals || dispatch)
                && let Some(bytes) = global_load(instruction, name)
            {
                accesses.push((instruction.offset, Access::Global { bytes }));
            } else if let Some(&(_, name)) = named.as_ref()
                && dispatch
                && let Some(bytes) = global_store(instruction, name)
            {
                // A dispatch's global stores reach the global loads' buffer, written back (D961).
                stores.insert(instruction.offset);
                accesses.push((instruction.offset, Access::Global { bytes }));
            }
            if !lanes.is_empty() {
                step_lanes(&mut lanes, &held, instruction, *named);
            }
            step_selections(&mut selected, &held, instruction, *named);
            sharing.step(instruction, *named);
            step(&mut held, instruction, *named);
        }
    }
    accesses.sort_by_key(|(offset, _)| *offset);
    let mut buffers = slots(
        accesses,
        &shared,
        (decode, encodings),
        (first_register, count),
    );
    // An atomic writes its buffer but reads it first, so only plain stores leave a buffer unread.
    let plain_stores: std::collections::BTreeSet<u32> = stores
        .iter()
        .copied()
        .filter(|offset| !atomics.contains(offset))
        .collect();
    buffers.stored_only = stored_only(&buffers.served, &plain_stores);
    buffers.stored = buffers
        .served
        .iter()
        .filter(|(offset, _)| stores.contains(offset))
        .map(|(_, &slot)| slot)
        .collect();
    buffers
}

/// Each image access of a dispatch, by byte offset, whose descriptor registers hold a record
/// selected from a table at run time (D983), for a stage whose `count` user-data words land in the
/// scalar registers from `first_register`.
#[must_use]
pub fn selected_images(
    decode: &Decode,
    encodings: &EncodingTable,
    (first_register, count): (u32, u32),
) -> BTreeMap<u32, SelectedImage> {
    let mut images = BTreeMap::new();
    each_traced_instruction(
        decode,
        encodings,
        (first_register, count),
        &mut |instruction, name, (held, selected)| {
            if let Some(image) = selected_image(instruction, name, (held, selected)) {
                images.insert(instruction.offset, image);
            }
        },
    );
    images
}

/// Each sample or gather of a dispatch, by byte offset, whose four sampler registers hold words the
/// trace names - constants, including those a scalar instruction folds from constants, user data,
/// or loads at fixed places - for a stage whose `count` user-data words land in the scalar
/// registers from `first_register`: the sampler the host reads for an image no table record or
/// user-data word pairs it with, as a selected image's is (D983, D995).
#[must_use]
pub fn traced_samplers(
    decode: &Decode,
    encodings: &EncodingTable,
    (first_register, count): (u32, u32),
) -> BTreeMap<u32, [DescriptorWord; 4]> {
    let mut samplers = BTreeMap::new();
    each_traced_instruction(
        decode,
        encodings,
        (first_register, count),
        &mut |instruction, name, (held, _)| {
            if (name.starts_with("image_sample") || name.starts_with("image_gather"))
                && let Some(Operand::Scalar(first)) = instruction.operands.get(3)
                && let Some(words) = descriptor(held, *first)
            {
                samplers.insert(instruction.offset, words);
            }
        },
    );
    samplers
}

/// What [`each_traced_instruction`] calls for each instruction: it, its name, and the registers'
/// traced words and selections before it.
type TracedVisit<'a> = dyn FnMut(&Instruction, &str, (&Held, &Selections)) + 'a;

/// Calls `visit` with each named instruction a dispatch reaches and the registers' traced words and
/// selections before it: the walk [`selected_images`] and [`traced_samplers`] share.
fn each_traced_instruction(
    decode: &Decode,
    encodings: &EncodingTable,
    (first_register, count): (u32, u32),
    visit: &mut TracedVisit<'_>,
) {
    let named = named_instructions(decode, encodings);
    let Ok(blocks) = crate::blocks::split(decode, |instruction| {
        let family = encodings
            .encodings()
            .get(usize::from(instruction.encoding?))?;
        Some(family.name.clone())
    }) else {
        return;
    };
    let reached = block_entries(decode, &named, &blocks, entry_state(first_register, count));
    let selections = selection_entries(decode, &named, (&blocks, &reached), true);
    for ((block, state), selected) in blocks.iter().zip(reached).zip(selections) {
        let (Some(mut held), Some(mut selected)) = (state, selected) else {
            continue;
        };
        for (instruction, named) in decode.instructions[block.first..block.end]
            .iter()
            .zip(&named[block.first..block.end])
        {
            if let Some(&(_, name)) = named.as_ref() {
                visit(instruction, name, (&held, &selected));
            }
            step_selections(&mut selected, &held, instruction, *named);
            step(&mut held, instruction, *named);
        }
    }
}

/// The table an image access's descriptor was selected from at run time, where it was (D983): a
/// sample, gather, fetch or size query whose eight descriptor registers hold, in order, the words
/// of one selecting load's record. A store is not one: nothing is written back to the images a
/// table names. The sampler is traced as a buffer descriptor's words are.
fn selected_image(
    instruction: &Instruction,
    name: &str,
    (held, selected): (&Held, &Selections),
) -> Option<SelectedImage> {
    if !name.starts_with("image_")
        || name.starts_with("image_store")
        || name.starts_with("image_atomic")
        || crate::model::bvh::is_intersect_ray(name)
    {
        return None;
    }
    let Some(Operand::Scalar(first)) = instruction.operands.get(2) else {
        return None;
    };
    let table = selected_record(selected, *first, crate::model::IMAGE_DESCRIPTOR_REGISTERS)?;
    let through_sampler = name.starts_with("image_sample") || name.starts_with("image_gather");
    let sampler = match instruction.operands.get(3) {
        Some(Operand::Scalar(first)) if through_sampler => descriptor(held, *first),
        _ => None,
    };
    Some(SelectedImage {
        table,
        sampler,
        sampled: through_sampler,
    })
}

/// The offset and name of the first write that keeps a program from being traced: any that is
/// not, where `stores` are traced, a buffer store, or in a `dispatch` an image access it binds whole
/// or a global store its global buffer takes (D961). A pixel shader's image store reaches a harness
/// image, not the guest's, and stays untraced.
fn untraced_by(
    decode: &Decode,
    named: &[Option<(&str, &str)>],
    (stores, dispatch): (bool, bool),
    skipped: &std::collections::BTreeSet<u32>,
) -> Option<(u32, String)> {
    decode
        .instructions
        .iter()
        .zip(named)
        .filter(|(instruction, _)| !skipped.contains(&instruction.offset))
        .find_map(|(instruction, named)| {
            let (_, name) = (*named)?;
            let bound = bound_buffer_store(name)
                || (dispatch
                    && (bound_image_access(name) || global_store(instruction, name).is_some()));
            (writes_memory(name) && !(stores && bound))
                .then(|| (instruction.offset, name.to_owned()))
        })
}

/// Each instruction's family and mnemonic, [`None`] for one the tables do not name.
fn named_instructions<'a>(
    decode: &Decode,
    encodings: &'a EncodingTable,
) -> Vec<Option<(&'a str, &'a str)>> {
    decode
        .instructions
        .iter()
        .map(|instruction| {
            let family = encodings
                .encodings()
                .get(usize::from(instruction.encoding?))?;
            Some((
                family.name.as_str(),
                encodings.mnemonic_for(&family.name, instruction.opcode)?,
            ))
        })
        .collect()
}

/// Where the descriptor a ray intersection (`image_bvh_intersect_ray`, `image_bvh64_intersect_ray`)
/// reads its hierarchy through comes from: its four scalar registers, traced as a buffer
/// descriptor's are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HierarchySource {
    /// The descriptor's words, from the stage's user data, constants and loads at fixed places.
    Descriptor([DescriptorWord; 4]),
    /// A descriptor the program computes, found per dispatch by running the program up to the
    /// intersection (D753); its scalar offset is `null`, which the instruction has none of.
    Computed(Box<Computed>),
}

/// The distinct descriptors a program's ray intersections read their hierarchies through, in
/// program order, for a stage whose `count` user-data words land in the scalar registers from
/// `first_register`. The intersection reads its nodes through the window, not a bound buffer, so
/// these are traced apart from [`trace`]'s slots and whatever else the program writes. An
/// intersection whose program prefix cannot be rebuilt is left out.
#[must_use]
pub fn trace_hierarchies(
    decode: &Decode,
    encodings: &EncodingTable,
    (first_register, count): (u32, u32),
) -> Vec<HierarchySource> {
    let named = named_instructions(decode, encodings);
    if !named
        .iter()
        .flatten()
        .any(|(_, name)| crate::model::bvh::is_intersect_ray(name))
    {
        return Vec::new();
    }
    let Ok(blocks) = crate::blocks::split(decode, |instruction| {
        let family = encodings
            .encodings()
            .get(usize::from(instruction.encoding?))?;
        Some(family.name.clone())
    }) else {
        return Vec::new();
    };
    let reached = block_entries(decode, &named, &blocks, entry_state(first_register, count));
    let mut sources = Vec::new();
    for (block, state) in blocks.iter().zip(reached) {
        let Some(mut held) = state else {
            continue;
        };
        for (instruction, named) in decode.instructions[block.first..block.end]
            .iter()
            .zip(&named[block.first..block.end])
        {
            if let Some((_, name)) = named
                && crate::model::bvh::is_intersect_ray(name)
                && let Some(Operand::Scalar(first)) = instruction.operands.get(2)
            {
                let source = match descriptor(&held, *first) {
                    Some(words) => Some(HierarchySource::Descriptor(words)),
                    None => prefix_words(decode, instruction.offset).map(|prefix| {
                        HierarchySource::Computed(Box::new(Computed {
                            prefix,
                            at: instruction.offset,
                            through: ComputedAccess::Descriptor {
                                first: *first,
                                reads: DescriptorReads::default(),
                            },
                            soffset: SOFFSET_NULL,
                            first_register,
                            user_data: count,
                            program_address: None,
                        }))
                    }),
                };
                if let Some(source) = source
                    && !sources.contains(&source)
                {
                    sources.push(source);
                }
            }
            step(&mut held, instruction, *named);
        }
    }
    sources
}

/// The high word every flat address at or above which no guest mapping holds: a guest runs in the
/// host's user address space, which ends below 2^47 (D989). A flat store there reaches the local
/// data share's or the private aperture ([`crate::device_address::LOCAL_APERTURE_HIGH`],
/// [`crate::device_address::PRIVATE_APERTURE_HIGH`]), or nothing, and never guest memory.
pub const BEYOND_GUEST_HIGH: u32 = 0x8000;

/// Whether `instruction`, named `named`, is a store in the flat segment: a flat-encoded store
/// (named as the global segment's, the family's mnemonics naming no segment) whose `SEG` field,
/// bits 15:14 of its first word, is zero. Its address may reach the local data share's or the
/// private aperture as well as guest memory (D964).
#[must_use]
pub fn flat_segment_store(instruction: &Instruction, named: Option<(&str, &str)>) -> bool {
    named.is_some_and(|(family, name)| {
        family == "FLAT"
            && name.starts_with("global_store_")
            && (instruction.word >> 14) & 3 == FLAT_SEGMENT
    })
}

/// A flat-encoded access's `SEG` for the flat segment, which routes by aperture.
const FLAT_SEGMENT: u32 = 0;

/// The byte offsets of a program's flat stores whose address's high word is at least
/// [`BEYOND_GUEST_HIGH`] in every lane, for a stage whose `count` user-data words land in the
/// scalar registers from `first_register` (D989): stores into the wave's own local data share or
/// private memory, which write no guest memory.
///
/// The high word is bounded where the same straight run of the program set it, after the last
/// change of the execution mask, with a `v_med3_u32` two of whose operands are constants or
/// scalar registers the trace holds as constants: a median is at least the smaller of any two of
/// its operands. Grand Theft Auto V's ray traversal clamps a stack pointer's high word between
/// [`crate::device_address::PRIVATE_APERTURE_HIGH`] and
/// [`crate::device_address::LOCAL_APERTURE_HIGH`] this way before each push (guest-observed, the
/// program at 0x74005b854a00). Any other write to a vector register forgets its bound.
#[must_use]
pub fn flat_stores_beyond_guest(
    decode: &Decode,
    encodings: &EncodingTable,
    (first_register, count): (u32, u32),
) -> std::collections::BTreeSet<u32> {
    let mut beyond = std::collections::BTreeSet::new();
    let named = named_instructions(decode, encodings);
    if !decode
        .instructions
        .iter()
        .zip(&named)
        .any(|(instruction, named)| flat_segment_store(instruction, *named))
    {
        return beyond;
    }
    let Ok(blocks) = crate::blocks::split(decode, |instruction| {
        let family = encodings
            .encodings()
            .get(usize::from(instruction.encoding?))?;
        Some(family.name.clone())
    }) else {
        return beyond;
    };
    let reached = block_entries(decode, &named, &blocks, entry_state(first_register, count));
    for (block, state) in blocks.iter().zip(reached) {
        let Some(mut held) = state else {
            continue;
        };
        // Each vector register's least value in every lane, where this run set one.
        let mut floors: BTreeMap<u16, u32> = BTreeMap::new();
        for (instruction, named) in decode.instructions[block.first..block.end]
            .iter()
            .zip(&named[block.first..block.end])
        {
            let Some((_, name)) = named else {
                floors.clear();
                step(&mut held, instruction, *named);
                continue;
            };
            let operands = instruction.operands.as_slice();
            if flat_segment_store(instruction, *named)
                && let Some(Operand::Vector(address)) = operands.first()
                && floors
                    .get(&(address + 1))
                    .is_some_and(|&floor| floor >= BEYOND_GUEST_HIGH)
            {
                beyond.insert(instruction.offset);
            }
            // A lane the mask leaves out keeps what it held before, so a change of the mask
            // forgets every bound.
            let masks = name.starts_with("v_cmpx")
                || name.contains("saveexec")
                || name.starts_with("s_wqm")
                || operands.iter().any(
                    |operand| matches!(operand, Operand::Named(special) if special.starts_with("exec")),
                );
            if masks || name.starts_with("v_movrel") || name.starts_with("s_set_gpr_idx") {
                floors.clear();
            } else {
                for operand in operands {
                    if let Operand::Vector(first) = operand {
                        floors.retain(|&register, _| {
                            !(*first..first.saturating_add(16)).contains(&register)
                        });
                    }
                }
            }
            if *name == "v_med3_u32"
                && let [Operand::Vector(destination), sources @ ..] = operands
            {
                let known: Vec<u32> = sources
                    .iter()
                    .take(3)
                    .filter_map(|source| match source {
                        Operand::Literal(value) => Some(*value),
                        Operand::Integer(value) => u32::try_from(*value).ok(),
                        Operand::Scalar(register) => {
                            match held.get(usize::from(*register)).copied().flatten() {
                                Some(DescriptorWord::Constant(value)) => Some(value),
                                _ => None,
                            }
                        }
                        _ => None,
                    })
                    .collect();
                if known.len() >= 2
                    && let Some(&floor) = known.iter().min()
                {
                    floors.insert(*destination, floor);
                }
            }
            step(&mut held, instruction, *named);
        }
    }
    beyond
}

/// The slots every access to which is one of `stores` (D809).
fn stored_only(
    served: &BTreeMap<u32, u32>,
    stores: &std::collections::BTreeSet<u32>,
) -> std::collections::BTreeSet<u32> {
    let slots: std::collections::BTreeSet<u32> = served.values().copied().collect();
    slots
        .into_iter()
        .filter(|&slot| {
            served
                .iter()
                .filter(|&(_, &served_slot)| served_slot == slot)
                .all(|(offset, _)| stores.contains(offset))
        })
        .collect()
}

/// The program's words before byte `at`, rebuilt from its instructions: every dword each one
/// occupies - its fixed part, a modifier dword, further address registers, a literal, and the
/// constant `v_madmk_f32` and `v_madak_f32` carry, which no literal operand names. [`None`] where an instruction's words do not account for its length.
fn prefix_words(decode: &Decode, at: u32) -> Option<Vec<u32>> {
    let mut words = Vec::new();
    for instruction in decode.instructions.iter().take_while(|i| i.offset < at) {
        if instruction.words.len() * 4 != instruction.length as usize {
            return None;
        }
        words.extend_from_slice(&instruction.words);
    }
    Some(words)
}

/// Gives each distinct descriptor or base a slot, in the order the program first reaches it, with
/// every access's reach through it combined.
fn slots(
    accesses: Vec<(u32, Access)>,
    shared: &BTreeMap<u32, u32>,
    (decode, encodings): (&Decode, &EncodingTable),
    (first_register, user_data): (u32, u32),
) -> DrawBuffers {
    let mut keys: Vec<Key> = Vec::new();
    let mut sources: Vec<BufferSource> = Vec::new();
    let mut buffers = DrawBuffers::default();
    let mut globals: Vec<(u32, u32)> = Vec::new();
    for (offset, access) in accesses {
        let (key, source) = match access {
            Access::Global { bytes } => {
                globals.push((offset, bytes));
                continue;
            }
            Access::Computed {
                through,
                soffset,
                table,
            } => {
                let Some(prefix) = prefix_words(decode, offset) else {
                    continue;
                };
                (
                    match table {
                        // Every access reading through a record of one table shares the table,
                        // wherever it is: each reads through the record it selected (D966).
                        Some(table) => Key::Table(table),
                        // One that shares an earlier access's descriptor is that access's (D952).
                        None => Key::Computed(shared.get(&offset).copied().unwrap_or(offset)),
                    },
                    BufferSource::Computed(Box::new(Computed {
                        prefix,
                        at: offset,
                        through,
                        soffset,
                        first_register,
                        user_data,
                        program_address: None,
                    })),
                )
            }
            Access::Descriptor(words, reads) => (
                Key::Descriptor(words),
                BufferSource::Descriptor { words, reads },
            ),
            Access::Pointer { base, end } => (
                Key::Pointer(base),
                BufferSource::Pointer { base, extent: end },
            ),
        };
        let earlier = keys.iter().position(|k| *k == key);
        let slot = if let Some(slot) = earlier {
            sources[slot] = combined(sources[slot].clone(), source);
            slot
        } else {
            keys.push(key);
            sources.push(source);
            sources.len() - 1
        };
        let slot = u32::try_from(slot).unwrap_or(u32::MAX);
        if let Key::Table(table) = key {
            buffers.tables.insert(slot, table);
            if earlier.is_some() {
                buffers.several.insert(slot);
            }
        }
        buffers.served.insert(offset, slot);
    }
    // A table's records are bound in the global buffer of pages (D966): a dispatch with no global
    // access of its own still has one, holding nothing else.
    if globals.is_empty() && !buffers.tables.is_empty() {
        sources.push(BufferSource::Global(Box::new(GlobalLoads {
            prefix: Vec::new(),
            loads: Vec::new(),
            first_register,
            user_data,
            width: 64,
        })));
    }
    // Every global load through one buffer, after the others: the run that finds their addresses
    // goes through the last of them.
    if let Some(&(last, _)) = globals.last()
        // Through the last load, and through any branch after it back to it: a loop's passes
        // all run (D883).
        && let Some(prefix) = prefix_words(
            decode,
            crate::evaluate::run_end(
                decode,
                encodings,
                &globals.iter().map(|&(at, _)| at).collect::<Vec<_>>(),
            )
            .max(last)
                + 1,
        )
    {
        let slot = u32::try_from(sources.len()).unwrap_or(u32::MAX);
        for &(offset, _) in &globals {
            buffers.served.insert(offset, slot);
        }
        sources.push(BufferSource::Global(Box::new(GlobalLoads {
            prefix,
            loads: globals,
            first_register,
            user_data,
            width: 64,
        })));
    }
    buffers.sources = sources;
    buffers
}

/// One buffer's source reaching what a further access through it reaches too.
fn combined(kept: BufferSource, more: BufferSource) -> BufferSource {
    match (kept, more) {
        (
            BufferSource::Descriptor { words, reads },
            BufferSource::Descriptor { reads: more, .. },
        ) => BufferSource::Descriptor {
            words,
            reads: reads.and(more),
        },
        (BufferSource::Pointer { base, extent }, BufferSource::Pointer { extent: more, .. }) => {
            BufferSource::Pointer {
                base,
                extent: extent.max(more),
            }
        }
        // Accesses through one computed descriptor or base reach what each does (D952).
        (BufferSource::Computed(mut kept), BufferSource::Computed(more)) => {
            kept.through = match (kept.through, more.through) {
                (
                    ComputedAccess::Descriptor { first, reads },
                    ComputedAccess::Descriptor { reads: also, .. },
                ) => ComputedAccess::Descriptor {
                    first,
                    reads: reads.and(also),
                },
                (
                    ComputedAccess::Pointer { base, end },
                    ComputedAccess::Pointer { end: also, .. },
                ) => ComputedAccess::Pointer {
                    base,
                    end: end.max(also),
                },
                (through, _) => through,
            };
            BufferSource::Computed(kept)
        }
        (kept, _) => kept,
    }
}

/// What makes two accesses read one buffer: the same descriptor words, or the same base. A computed
/// one is its own, by its offset, or the earlier access's whose descriptor it reads (D952).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Descriptor([DescriptorWord; 4]),
    Pointer(TableBase),
    Computed(u32),
    Table(SelectedTable),
}

/// What a computed access reads (D952): whether through a base pair rather than a descriptor, its
/// first register, its scalar offset's code, and for each register it reads - the descriptor's or
/// the pair's, then the offset's - one past the byte of the instruction in its block that last may
/// have written it, zero where none in the block has.
type Reading = (bool, u16, u8, [u32; 5]);

/// The computed accesses of one block, as [`trace_with`] walks it (D952). Two that read the same
/// registers, which no instruction between them may have written, read the same words: in one block
/// the second runs exactly when the first has, with nothing in between, so the descriptor a run up
/// to the first finds is the second's too, and one buffer serves both.
struct ComputedSharing {
    /// For each scalar register, one past the byte of the instruction in the block that last may
    /// have written it; zero where none has.
    written: Vec<u32>,
    /// Each earlier access's [`Reading`], with its byte.
    seen: Vec<(Reading, u32)>,
}

impl ComputedSharing {
    fn new() -> Self {
        Self {
            written: vec![0; crate::model::SCALAR_REGISTERS as usize],
            seen: Vec::new(),
        }
    }

    /// Records the computed access at byte `at`, answering the earlier access in the block it reads
    /// the same words as, if there is one. One whose scalar offset is a register no tracing holds -
    /// `m0`, a lane mask - is its own.
    fn reads(&mut self, at: u32, through: ComputedAccess, soffset: u8) -> Option<u32> {
        let (pointer, first, count) = match through {
            ComputedAccess::Descriptor { first, .. } => (false, first, 4),
            ComputedAccess::Pointer { base, .. } => (true, base, 2),
        };
        let mut writers = [0; 5];
        for (writer, register) in writers.iter_mut().zip(first..first + count) {
            *writer = *self.written.get(usize::from(register))?;
        }
        match soffset {
            register if u32::from(register) < crate::model::SCALAR_REGISTERS => {
                writers[4] = *self.written.get(usize::from(register))?;
            }
            // `null`, and an inline constant, read no register.
            SOFFSET_NULL | 128.. => {}
            _ => return None,
        }
        let reading = (pointer, first, soffset, writers);
        if let Some(&(_, earlier)) = self.seen.iter().find(|(seen, _)| *seen == reading) {
            return Some(earlier);
        }
        self.seen.push((reading, at));
        None
    }

    /// Notes the scalar registers an instruction may write.
    fn step(&mut self, instruction: &Instruction, named: Option<(&str, &str)>) {
        for register in written_registers(instruction, named) {
            if let Some(writer) = self.written.get_mut(register) {
                *writer = instruction.offset + 1;
            }
        }
    }
}

/// The scalar registers an instruction may write, as [`step`] carries them: every register it
/// changes from a state where each holds a value no instruction forms but by moving it - a move
/// from a register to itself writes the value it held, and is no write.
fn written_registers(instruction: &Instruction, named: Option<(&str, &str)>) -> Vec<usize> {
    let marked: Held = (0..crate::model::SCALAR_REGISTERS)
        .map(|register| Some(DescriptorWord::UserData(u32::MAX - register)))
        .collect();
    let mut after = marked.clone();
    step(&mut after, instruction, named);
    marked
        .iter()
        .zip(&after)
        .enumerate()
        .filter(|(_, (before, now))| before != now)
        .map(|(register, _)| register)
        .collect()
}

/// What each scalar register holds, where the program's own writes fix it: [`None`] where it could
/// hold more than one thing.
type Held = Vec<Option<DescriptorWord>>;

/// Which vector registers hold a record index the draw bounds (D879): [`None`] for any other.
type Lanes = Vec<Option<RecordIndex>>;

/// The vector registers a primitive shader's vertex thread finds its vertex and instance ids in
/// (the module's seeding, `wavefront.rs`; D863).
const VERTEX_ID: usize = 5;
const INSTANCE_ID: usize = 8;

/// One traced access, before its base's reach is known.
#[derive(Debug, Clone, Copy)]
enum Access {
    /// Through a buffer descriptor, reaching as far as the reads say.
    Descriptor([DescriptorWord; 4], DescriptorReads),
    /// A scalar load at a base, reaching `end` bytes past it.
    Pointer { base: TableBase, end: u32 },
    /// Through a descriptor or base, or with a scalar offset, the program computes (D753).
    Computed {
        through: ComputedAccess,
        soffset: u8,
        /// The table a dispatch selected the descriptor from at run time, where it did (D966).
        table: Option<SelectedTable>,
    },
    /// A global load with no scalar base, reading `bytes` from its address (D758).
    Global { bytes: u32 },
}

/// How many bytes a global load with no scalar base reads from its address, its immediate offset
/// counted in the address - a byte or a half for the sub-word loads (D878); [`None`] for any other
/// instruction.
fn global_load(instruction: &Instruction, name: &str) -> Option<u32> {
    let bytes = match name {
        "global_load_ubyte" | "global_load_sbyte" => 1,
        "global_load_ushort" | "global_load_sshort" => 2,
        _ => match name.strip_prefix("global_load_dword")? {
            "" => 4,
            count => 4 * count.strip_prefix('x')?.parse::<u32>().ok()?,
        },
    };
    match instruction.operands.get(2)? {
        Operand::Named(base) if base == crate::model::FLAT_NO_BASE => Some(bytes),
        _ => None,
    }
}

/// The bytes a global store with no scalar base writes, for the stores a dispatch's global buffer
/// takes (D961): whole words, one to four, or a byte or a half, which merges into the word holding
/// it as one step a part, keeping what another lane's store writes of the rest (D977).
fn global_store(instruction: &Instruction, name: &str) -> Option<u32> {
    let bytes = match name {
        "global_store_byte" => 1,
        "global_store_short" => 2,
        _ => match name.strip_prefix("global_store_dword")? {
            "" => 4,
            "x2" => 8,
            "x3" => 12,
            "x4" => 16,
            _ => return None,
        },
    };
    match instruction.operands.get(2)? {
        Operand::Named(base) if base == crate::model::FLAT_NO_BASE => Some(bytes),
        _ => None,
    }
}

/// The registers at entry: each user-data word where the hardware loads it, and nothing else known.
fn entry_state(first_register: u32, count: u32) -> Held {
    let mut held: Held = vec![None; crate::model::SCALAR_REGISTERS as usize];
    for index in 0..count {
        if let Some(word) = held.get_mut((first_register + index) as usize) {
            *word = Some(DescriptorWord::UserData(index));
        }
    }
    held
}

/// What each reachable block starts with, found by carrying states along every branch until they
/// settle; a register two paths disagree on is unknown. [`None`] for a block nothing reaches.
fn block_entries(
    decode: &Decode,
    named: &[Option<(&str, &str)>],
    blocks: &[crate::blocks::Block],
    entry: Held,
) -> Vec<Option<Held>> {
    use crate::blocks::block_at;
    let mut entries: Vec<Option<Held>> = vec![None; blocks.len()];
    let Some(first) = entries.first_mut() else {
        return entries;
    };
    *first = Some(entry);
    let mut pending = vec![0usize];
    while let Some(index) = pending.pop() {
        let (Some(block), Some(Some(start))) = (blocks.get(index), entries.get(index)) else {
            continue;
        };
        let mut held = start.clone();
        for (instruction, named) in decode.instructions[block.first..block.end]
            .iter()
            .zip(&named[block.first..block.end])
        {
            step(&mut held, instruction, *named);
        }
        for successor in successors(block)
            .into_iter()
            .filter_map(|at| block_at(blocks, at))
        {
            let changed = match &mut entries[successor] {
                slot @ None => {
                    *slot = Some(held.clone());
                    true
                }
                Some(known) => meet(known, &held),
            };
            if changed {
                pending.push(successor);
            }
        }
    }
    entries
}

/// Where a block's control goes next.
fn successors(block: &crate::blocks::Block) -> Vec<u32> {
    use crate::blocks::Terminator;
    match block.terminator {
        Terminator::End => Vec::new(),
        Terminator::Jump { target } => vec![target],
        Terminator::Branch {
            target,
            fallthrough,
        } => vec![target, fallthrough],
        Terminator::Fallthrough { next } => vec![next],
    }
}

/// Which vector registers each reachable block starts with an id in (D879), carried along every
/// branch until they settle from a primitive shader's entry, where `v5` holds the vertex id and
/// `v8` the instance id; a register two paths disagree on holds none. The scalar registers each
/// block starts with, `scalars`, have settled already.
fn lane_entries(
    decode: &Decode,
    named: &[Option<(&str, &str)>],
    blocks: &[crate::blocks::Block],
    scalars: &[Option<Held>],
) -> Vec<Option<Lanes>> {
    let mut entries: Vec<Option<Lanes>> = vec![None; blocks.len()];
    let Some(first) = entries.first_mut() else {
        return entries;
    };
    let mut entry: Lanes = vec![None; 256];
    entry[VERTEX_ID] = Some(RecordIndex::Vertex(None));
    entry[INSTANCE_ID] = Some(RecordIndex::Instance(None));
    *first = Some(entry);
    let mut pending = vec![0usize];
    while let Some(index) = pending.pop() {
        let (Some(block), Some(Some(start)), Some(Some(held))) =
            (blocks.get(index), entries.get(index), scalars.get(index))
        else {
            continue;
        };
        let (mut lanes, mut held) = (start.clone(), held.clone());
        for (instruction, named) in decode.instructions[block.first..block.end]
            .iter()
            .zip(&named[block.first..block.end])
        {
            step_lanes(&mut lanes, &held, instruction, *named);
            step(&mut held, instruction, *named);
        }
        for successor in successors(block)
            .into_iter()
            .filter_map(|at| crate::blocks::block_at(blocks, at))
        {
            let changed = match &mut entries[successor] {
                slot @ None => {
                    *slot = Some(lanes.clone());
                    true
                }
                Some(known) => meet_lanes(known, &lanes),
            };
            if changed {
                pending.push(successor);
            }
        }
    }
    entries
}

/// Narrows `known` to what it shares with `other`, answering whether anything changed.
fn meet_lanes(known: &mut Lanes, other: &Lanes) -> bool {
    let mut changed = false;
    for (index, theirs) in known.iter_mut().zip(other) {
        if index.is_some() && *index != *theirs {
            *index = None;
            changed = true;
        }
    }
    changed
}

/// Carries `lanes` over one instruction (D879).
///
/// A `v_mov_b32` from a register copies what it holds, and a `v_add_nc_u32` of an id with nothing
/// added and a traced scalar register or a constant holds the id plus that word. Any other write
/// to a vector register leaves it holding none: the first operand of every vector-writing family
/// is its destination, as wide as [`vector_destination_words`] says; a swap writes its second too;
/// and an instruction with no name, or a relative move, leaves none holding anything.
fn step_lanes(
    lanes: &mut Lanes,
    held: &Held,
    instruction: &Instruction,
    named: Option<(&str, &str)>,
) {
    let Some((_, name)) = named else {
        lanes.fill(None);
        return;
    };
    if name.starts_with("v_movrel") || name.starts_with("v_swaprel") {
        lanes.fill(None);
        return;
    }
    let operands = instruction.operands.as_slice();
    let lane = |operand: &Operand| match operand {
        Operand::Vector(register) => lanes.get(usize::from(*register)).copied().flatten(),
        _ => None,
    };
    let word = |operand: &Operand| match operand {
        Operand::Scalar(register) => held.get(usize::from(*register)).copied().flatten(),
        Operand::Integer(value) => i32::try_from(*value)
            .ok()
            .map(|v| DescriptorWord::Constant(u32::from_ne_bytes(v.to_ne_bytes()))),
        Operand::Literal(value) => Some(DescriptorWord::Constant(*value)),
        _ => None,
    };
    let plus = |id: RecordIndex, added: Option<DescriptorWord>| match (id, added?) {
        (RecordIndex::Vertex(None), added) => Some(RecordIndex::Vertex(Some(added))),
        (RecordIndex::Instance(None), added) => Some(RecordIndex::Instance(Some(added))),
        _ => None,
    };
    // The short and long encodings of one operation do the same.
    let operation = name
        .strip_suffix("_e32")
        .or_else(|| name.strip_suffix("_e64"))
        .unwrap_or(name);
    let written = match (operation, operands) {
        ("v_mov_b32", [Operand::Vector(destination), source]) => Some((*destination, lane(source))),
        ("v_add_nc_u32", [Operand::Vector(destination), a, b]) => Some((
            *destination,
            lane(b)
                .and_then(|id| plus(id, word(a)))
                .or_else(|| lane(a).and_then(|id| plus(id, word(b)))),
        )),
        _ => None,
    };
    if let Some(Operand::Vector(first)) = operands.first() {
        for index in lanes
            .iter_mut()
            .skip(usize::from(*first))
            .take(vector_destination_words(name))
        {
            *index = None;
        }
    }
    if name.starts_with("v_swap")
        && let Some(Operand::Vector(second)) = operands.get(1)
        && let Some(index) = lanes.get_mut(usize::from(*second))
    {
        *index = None;
    }
    if let Some((destination, value)) = written
        && let Some(index) = lanes.get_mut(usize::from(destination))
    {
        *index = value;
    }
}

/// How many vector registers an instruction's first operand may span as a destination, at least:
/// one for a 32-bit vector ALU result, two for any naming a 64-bit type, four for the quad-SAD; a
/// memory read's dwords or channels, one more for the status word `tfe` returns; five for an
/// image's four channels and status. Where it is a source instead, forgetting it costs only a
/// bound.
fn vector_destination_words(name: &str) -> usize {
    if let Some(alu) = name.strip_prefix("v_") {
        return if alu.contains("mqsad_u32") {
            4
        } else if alu.contains("64") {
            2
        } else {
            1
        };
    }
    if name.starts_with("image_") {
        return 5;
    }
    let has = |parts: &[&str]| parts.iter().any(|part| name.contains(part));
    let data = if has(&["read2_b64", "read2st64_b64", "b128", "x4", "xyzw"]) {
        4
    } else if has(&["b96", "x3", "xyz"]) {
        3
    } else if has(&["b64", "x2", "xy", "read2"]) {
        2
    } else {
        1
    };
    data + 1
}

/// `access` with what its record index is, where it indexes records by a register `lanes` holds an
/// id in (D879). The index is the address operand's first register.
fn indexed_by(mut access: Access, instruction: &Instruction, lanes: &Lanes) -> Access {
    let index = match instruction.operands.get(1) {
        Some(Operand::Vector(register)) => lanes.get(usize::from(*register)).copied().flatten(),
        _ => None,
    };
    if let Access::Descriptor(_, reads)
    | Access::Computed {
        through: ComputedAccess::Descriptor { reads, .. },
        ..
    } = &mut access
        && reads.indexed
    {
        reads.index = index;
    }
    access
}

/// What a scalar register holds of a descriptor a dispatch selected from a table at run time
/// (D966): the table, and which of the descriptor's four words.
type Selection = (SelectedTable, u8);

/// Each scalar register's [`Selection`], [`None`] for one holding anything else.
type Selections = Vec<Option<Selection>>;

/// The table and destination of a load selecting a descriptor from a table at run time (D966):
/// `s_buffer_load_dwordx4` through a traced descriptor at an offset register, neither `null` nor an
/// inline constant, which the program computes. Its immediate is where the descriptor lies in the
/// record the offset selects.
fn selecting_load(
    instruction: &Instruction,
    name: &str,
    held: &Held,
) -> Option<(u16, SelectedTable)> {
    // Four words of a buffer descriptor, or eight of an image descriptor (D983).
    if !matches!(name, "s_buffer_load_dwordx4" | "s_buffer_load_dwordx8") {
        return None;
    }
    let [
        Operand::Scalar(destination),
        Operand::Scalar(base),
        Operand::Immediate(offset),
    ] = instruction.operands.as_slice()
    else {
        return None;
    };
    let code = (instruction.second_word? >> 25) & 0x7f;
    if code >= u32::from(SOFFSET_NULL) || !solved_scalar_offset(instruction) {
        return None;
    }
    let words = descriptor(held, *base)?;
    let element = u32::try_from(*offset).ok()?;
    Some((*destination, SelectedTable { words, element }))
}

/// The table the four registers from `first` hold a selected descriptor of, in order (D966).
fn selected_table(selected: &Selections, first: u16) -> Option<SelectedTable> {
    selected_record(selected, first, 4)
}

/// The table the `count` registers from `first` hold a selected record's first words of, in order:
/// four of a buffer descriptor (D966), eight of an image descriptor (D983).
fn selected_record(selected: &Selections, first: u16, count: u32) -> Option<SelectedTable> {
    let mut table = None;
    for word in 0..u8::try_from(count).ok()? {
        let (held, at) = selected
            .get(usize::from(first) + usize::from(word))
            .copied()
            .flatten()?;
        if at != word || table.is_some_and(|table| table != held) {
            return None;
        }
        table = Some(held);
    }
    table
}

/// `access` with the table its descriptor was selected from at run time, where it reads through
/// one (D966): a vector access that adds no scalar offset, whose reach the record's own descriptor
/// then bounds, or a scalar buffer load, which reads within the record's descriptor whatever its
/// offset - at an offset register, every byte of it.
fn selected_by(mut access: Access, name: &str, selected: &Selections) -> Access {
    let Access::Computed {
        through: ComputedAccess::Descriptor { first, reads },
        soffset,
        table,
    } = &mut access
    else {
        return access;
    };
    let scalar = name.starts_with("s_buffer_load");
    // `null`, or the inline constant zero.
    let no_offset = *soffset == SOFFSET_NULL || *soffset == 128;
    if !scalar && !no_offset {
        return access;
    }
    *table = selected_table(selected, *first);
    if scalar && table.is_some() && *soffset != SOFFSET_NULL {
        reads.scalar_reach = u32::MAX;
    }
    access
}

/// Which scalar registers each reachable block starts with a selected descriptor's words in
/// (D966), carried along every branch until they settle; a register two paths disagree on holds
/// none. The scalar registers each block starts with, `scalars`, have settled already. Only a
/// `dispatch` selects so; any other program's blocks hold none.
fn selection_entries(
    decode: &Decode,
    named: &[Option<(&str, &str)>],
    (blocks, scalars): (&[crate::blocks::Block], &[Option<Held>]),
    dispatch: bool,
) -> Vec<Option<Selections>> {
    let mut entries: Vec<Option<Selections>> = vec![None; blocks.len()];
    let Some(first) = entries.first_mut().filter(|_| dispatch) else {
        return entries;
    };
    *first = Some(vec![None; crate::model::SCALAR_REGISTERS as usize]);
    let mut pending = vec![0usize];
    while let Some(index) = pending.pop() {
        let (Some(block), Some(Some(start)), Some(Some(held))) =
            (blocks.get(index), entries.get(index), scalars.get(index))
        else {
            continue;
        };
        let (mut selected, mut held) = (start.clone(), held.clone());
        for (instruction, named) in decode.instructions[block.first..block.end]
            .iter()
            .zip(&named[block.first..block.end])
        {
            step_selections(&mut selected, &held, instruction, *named);
            step(&mut held, instruction, *named);
        }
        for successor in successors(block)
            .into_iter()
            .filter_map(|at| crate::blocks::block_at(blocks, at))
        {
            let changed = match &mut entries[successor] {
                slot @ None => {
                    *slot = Some(selected.clone());
                    true
                }
                Some(known) => {
                    let mut changed = false;
                    for (word, theirs) in known.iter_mut().zip(&selected) {
                        if word.is_some() && *word != *theirs {
                            *word = None;
                            changed = true;
                        }
                    }
                    changed
                }
            };
            if changed {
                pending.push(successor);
            }
        }
    }
    entries
}

/// Carries `selected` over one instruction (D966): a selecting load fills its four registers, a
/// scalar move copies what its source holds, and every other register an instruction may write
/// holds none, as [`written_registers`] finds them. A block no selection reaches carries none.
fn step_selections(
    selected: &mut Selections,
    held: &Held,
    instruction: &Instruction,
    named: Option<(&str, &str)>,
) {
    if selected.is_empty() {
        return;
    }
    let at = |register: u16| selected.get(usize::from(register)).copied().flatten();
    let mut set: Vec<(u16, Option<Selection>)> = Vec::new();
    match (named.map(|(_, name)| name), instruction.operands.as_slice()) {
        (Some("s_mov_b32"), [Operand::Scalar(destination), Operand::Scalar(source)]) => {
            set.push((*destination, at(*source)));
        }
        (Some("s_mov_b64"), [Operand::Scalar(destination), Operand::Scalar(source)]) => {
            set.push((*destination, at(*source)));
            set.push((destination + 1, at(source + 1)));
        }
        (Some(name), _) => {
            if let Some((destination, table)) = selecting_load(instruction, name, held) {
                let words = u8::try_from(scalar_words(name)).unwrap_or(4);
                for word in 0..words {
                    set.push((destination + u16::from(word), Some((table, word))));
                }
            }
        }
        (None, _) => {}
    }
    for register in written_registers(instruction, named) {
        if let Some(word) = selected.get_mut(register) {
            *word = None;
        }
    }
    for (register, value) in set {
        if let Some(word) = selected.get_mut(usize::from(register)) {
            *word = value;
        }
    }
}

/// Narrows `known` to what it shares with `other`, answering whether anything changed.
fn meet(known: &mut Held, other: &Held) -> bool {
    let mut changed = false;
    for (word, theirs) in known.iter_mut().zip(other) {
        if word.is_some() && *word != *theirs {
            *word = None;
            changed = true;
        }
    }
    changed
}

/// Whether an instruction writes memory a draw's bound buffers snapshot. The local data share is
/// the wavefront's own.
fn writes_memory(name: &str) -> bool {
    (name.contains("store") || name.contains("atomic")) && !name.starts_with("ds_")
}

/// Whether an instruction is a buffer store a dispatch's bound buffer takes (D746): an untyped one,
/// or one converting by its descriptor's format (D738), which addresses the buffer the same way.
fn bound_buffer_store(name: &str) -> bool {
    name.starts_with("buffer_store_dword")
        || matches!(name, "buffer_store_byte" | "buffer_store_short")
        || name.starts_with("buffer_store_format_")
        || name.starts_with("buffer_atomic_")
}

/// Whether an instruction reaches an image a dispatch binds whole and writes back exactly, as it
/// does its buffers (D746): such an access needs no window and keeps no buffer from being traced.
/// A size query asks the image a load would read, and reads no memory at all; a sample at a named
/// level or by gradients reads the image a fetch would, through its sampler (D904), and so does a
/// gather (D913). A depth-compare sample at level zero reads it too, the comparison made in the
/// module, since a dispatch's images are bound as their texels (D976).
pub(crate) fn bound_image_access(name: &str) -> bool {
    matches!(
        name,
        "image_load"
            | "image_load_mip"
            | "image_store"
            | "image_store_mip"
            | "image_get_resinfo"
            | "image_sample_lz"
            | "image_sample_lz_o"
            | "image_sample_c_lz"
            | "image_sample_l"
            | "image_sample_d"
            | "image_gather4_lz"
    )
}

/// `null` in a scalar memory instruction's `SOFFSET` field, bits 31:25 of its second word: no
/// offset register.
const SOFFSET_NULL: u8 = 125;

/// Whether a scalar memory access adds nothing to its immediate: no offset register, and no offset
/// bits above the sixteen the operand layout gives. Any other is left untraced.
fn plain_scalar_offset(instruction: &Instruction) -> bool {
    let second = instruction.second_word.unwrap_or(0);
    (second >> 25) & 0x7f == u32::from(SOFFSET_NULL) && solved_scalar_offset(instruction)
}

/// Whether a scalar memory access's immediate sets no offset bits above the sixteen the operand
/// layout gives, whatever offset register it adds.
fn solved_scalar_offset(instruction: &Instruction) -> bool {
    (instruction.second_word.unwrap_or(0) >> 16).trailing_zeros() >= 5
}

/// How many registers a scalar memory access fills, from the width its name states.
fn scalar_words(name: &str) -> u32 {
    match name.rsplit_once("dwordx") {
        Some((_, count)) => count.parse().unwrap_or(16),
        None => 1,
    }
}

/// The four words of the descriptor in `first..first + 4`, when all four are traced.
fn descriptor(held: &Held, first: u16) -> Option<[DescriptorWord; 4]> {
    let at = |step: u16| held.get(usize::from(first + step)).copied().flatten();
    Some([at(0)?, at(1)?, at(2)?, at(3)?])
}

/// The base a scalar load reads from, when both halves are the draw's words or constants.
fn table_base(held: &Held, first: u16) -> Option<TableBase> {
    let half = |step: u16| match held.get(usize::from(first + step)).copied().flatten()? {
        DescriptorWord::UserData(index) => Some(TableWord::UserData(index)),
        DescriptorWord::Constant(value) => Some(TableWord::Constant(value)),
        DescriptorWord::Loaded { .. } => None,
    };
    Some(TableBase {
        low: half(0)?,
        high: half(1)?,
    })
}

/// The traced access an instruction makes, if it reads through a buffer and its descriptor or base
/// is traced. In a `dispatch`, a scalar buffer load through a traced descriptor at an offset the
/// program computes reaches the descriptor's whole buffer (D966).
fn access(instruction: &Instruction, name: &str, held: &Held, dispatch: bool) -> Option<Access> {
    let operands = instruction.operands.as_slice();
    if name.starts_with("s_load_dword") || name.starts_with("s_buffer_load_dword") {
        let [_, Operand::Scalar(base), Operand::Immediate(offset)] = operands else {
            return None;
        };
        let end = u32::try_from(*offset)
            .ok()?
            .checked_add(scalar_words(name) * 4)?;
        // A load from a base the program computes, or adding a scalar offset, is found per draw
        // (D753): the offset field is the second word's bits 31:25.
        let table = table_base(held, *base).filter(|_| plain_scalar_offset(instruction));
        if name.starts_with("s_load_dword") && table.is_none() {
            return Some(Access::Computed {
                through: ComputedAccess::Pointer { base: *base, end },
                soffset: u8::try_from((instruction.second_word? >> 25) & 0x7f).ok()?,
                table: None,
            });
        }
        if name.starts_with("s_buffer_load") && solved_scalar_offset(instruction) {
            let reads = DescriptorReads {
                scalar_reach: end,
                ..DescriptorReads::default()
            };
            // A descriptor from the user data, read at its immediate alone, is the draw's as it
            // stands; an offset register's value, or a descriptor the program computes or loads, is
            // found per draw through the program before the access (D753), the binding reaching the
            // immediate's words past the offset.
            return match descriptor(held, *base) {
                Some(words) if plain_scalar_offset(instruction) => {
                    Some(Access::Descriptor(words, reads))
                }
                // A dispatch's load at an offset it computes through a descriptor it does not:
                // the load reads zero past the descriptor's records whatever the offset, so the
                // records are every byte it can read, and every such load through the descriptor
                // shares them (D966). A table a record is selected from is read this way.
                Some(words) if dispatch => Some(Access::Descriptor(
                    words,
                    DescriptorReads {
                        scalar_reach: u32::MAX,
                        ..DescriptorReads::default()
                    },
                )),
                _ => Some(Access::Computed {
                    through: ComputedAccess::Descriptor {
                        first: *base,
                        reads,
                    },
                    soffset: u8::try_from((instruction.second_word? >> 25) & 0x7f).ok()?,
                    table: None,
                }),
            };
        }
        if !plain_scalar_offset(instruction) {
            return None;
        }
        return Some(Access::Pointer {
            base: table_base(held, *base)?,
            end,
        });
    }
    if name.starts_with("buffer_") || name.starts_with("tbuffer_") {
        // The scalar offset is added to the address and not to the range check, so only an access
        // that adds none is bounded by what the descriptor says.
        let [_, _, Operand::Scalar(first), soffset] = operands else {
            return None;
        };
        // The addressing modifiers: the immediate offset in bits 11:0, `offen` at 12, `idxen` at 13.
        let word = instruction.word;
        let reach =
            (word & (1 << 12) == 0).then(|| (word & 0xfff) + vector_payload(instruction, name));
        let reads = DescriptorReads {
            scalar_reach: 0,
            vector: true,
            vector_reach: reach,
            indexed: word & (1 << 13) != 0,
            index: None,
        };
        let no_offset =
            matches!(soffset, Operand::Integer(0)) || *soffset == Operand::Named("null".into());
        // The scalar offset is added to the address and not to the range check, so a traced
        // descriptor bounds only an access that adds none; any other is found per draw (D753),
        // the offset field from the second word's bits 31:24.
        return match descriptor(held, *first) {
            Some(words) if no_offset => Some(Access::Descriptor(words, reads)),
            _ => Some(Access::Computed {
                through: ComputedAccess::Descriptor {
                    first: *first,
                    reads,
                },
                soffset: u8::try_from(instruction.second_word? >> 24).ok()?,
                table: None,
            }),
        };
    }
    None
}

/// How many bytes a vector buffer access reads or writes at its address: its dwords for an
/// untyped one, its element for a typed one, and sixteen - the widest - where neither is known.
fn vector_payload(instruction: &Instruction, name: &str) -> u32 {
    const WIDEST: u32 = 16;
    if name.starts_with("tbuffer_") {
        static FORMATS: std::sync::OnceLock<orbistoun_shader::FormatTable> =
            std::sync::OnceLock::new();
        let formats =
            FORMATS.get_or_init(|| orbistoun_shader::FormatTable::builtin().unwrap_or_default());
        return formats
            .get(orbistoun_shader::FormatTable::field(instruction.word))
            .map_or(WIDEST, |format| {
                format.widths.iter().sum::<u32>().div_ceil(8)
            });
    }
    // A format access's element is its descriptor's, unknown here, but each channel it names moves
    // at most one thirty-two-bit component.
    if let Some((_, channels)) = name.rsplit_once("_format_")
        && !channels.is_empty()
        && "xyzw".starts_with(channels)
    {
        return 4 * u32::try_from(channels.len()).unwrap_or(4);
    }
    // An atomic moves one word.
    if name.starts_with("buffer_atomic_") {
        return 4;
    }
    match name.rsplit_once("dword") {
        Some((_, "")) => 4,
        Some((_, count)) => count
            .strip_prefix('x')
            .and_then(|n| n.parse::<u32>().ok())
            .map_or(WIDEST, |n| n * 4),
        None => WIDEST,
    }
}

/// Forgets `count` registers from `first`.
fn forget(held: &mut Held, first: u16, count: u32) {
    for word in held
        .iter_mut()
        .skip(usize::from(first))
        .take(count as usize)
    {
        *word = None;
    }
}

/// How many scalar registers an instruction's scalar destination spans: two for a lane mask a
/// compare writes, and for a 64-bit destination - the first type in the name is the destination's
/// (`s_bfe_u64` and `s_cselect_b64` write a pair, `s_bcnt1_i32_b64` one); one otherwise.
fn destination_words(name: &str) -> u32 {
    if name.starts_with("v_cmp") {
        return 2;
    }
    let first_type = name.split('_').find(|part| {
        matches!(part.as_bytes(), [b'b' | b'u' | b'i' | b'f', rest @ ..]
            if !rest.is_empty() && rest.iter().all(u8::is_ascii_digit))
    });
    if first_type.is_some_and(|kind| kind.ends_with("64")) {
        2
    } else {
        1
    }
}

/// Carries `held` over a 64-bit move of a register pair: each word what its source holds.
fn moved_pair(held: &mut Held, destination: u16, source: u16) {
    let pair = [0, 1].map(|step| held.get(usize::from(source + step)).copied().flatten());
    for (step, value) in (0..).zip(pair) {
        if let Some(slot) = held.get_mut(usize::from(destination + step)) {
            *slot = value;
        }
    }
}

/// Carries `held` over a scalar ALU instruction every register source of which holds a constant
/// (D983): run through the translator's own semantics ([`crate::evaluate::fold_scalar`]), its
/// destination holds the constant the run leaves there - how a program builds a sampler descriptor
/// in place, with `s_bfm_b32`, `s_brev_b32`, `s_bitreplicate_b64_b32` and 64-bit moves of literals.
/// A source the trace does not hold as a constant, a special register among them, leaves the
/// destination as the caller forgot it.
fn fold_constants(held: &mut Held, instruction: &Instruction, (family, name): (&str, &str)) {
    static ENCODINGS: std::sync::OnceLock<Option<EncodingTable>> = std::sync::OnceLock::new();
    let [Operand::Scalar(destination), sources @ ..] = instruction.operands.as_slice() else {
        return;
    };
    if !matches!(family, "SOP1" | "SOP2" | "SOPK") {
        return;
    }
    let constant = |register: u16| match held.get(usize::from(register)).copied().flatten() {
        Some(DescriptorWord::Constant(value)) => Some((register, value)),
        _ => None,
    };
    let mut known = Vec::new();
    for source in sources {
        match source {
            Operand::Scalar(register) => {
                known.push(constant(*register).ok_or(()));
                // The second word of a pair source, where it is one.
                known.extend(constant(register + 1).map(Ok));
            }
            Operand::Integer(_) | Operand::Literal(_) | Operand::Immediate(_) => {}
            Operand::Named(float) if float.parse::<f32>().is_ok() => {}
            _ => return,
        }
    }
    let Ok(known) = known.into_iter().collect::<Result<Vec<_>, ()>>() else {
        return;
    };
    let Some(encodings) = ENCODINGS
        .get_or_init(|| EncodingTable::builtin().ok())
        .as_ref()
    else {
        return;
    };
    let Some(scalars) = crate::evaluate::fold_scalar(encodings, instruction, &known) else {
        return;
    };
    for step in 0..destination_words(name) as usize {
        let at = usize::from(*destination) + step;
        if let (Some(slot), Some(value)) = (held.get_mut(at), scalars.get(at).copied().flatten()) {
            *slot = Some(DescriptorWord::Constant(value));
        }
    }
}

/// Carries `held` over one instruction.
///
/// A move from a constant or a traced register, and a scalar load from a traced base, are followed.
/// Every other scalar register an instruction can write is forgotten, as wide as the widest
/// operand it can be: the destination of the scalar families; the destination of the vector ALU
/// families, which is a scalar one for a compare or a lane read; and the second operand of the
/// carry-out, divide-scale and 64-bit multiply-add forms, which write a scalar there. An
/// instruction with no name, or an indirect move, leaves nothing known.
fn step(held: &mut Held, instruction: &Instruction, named: Option<(&str, &str)>) {
    let Some((family, name)) = named else {
        held.fill(None);
        return;
    };
    let operands = instruction.operands.as_slice();
    if name.starts_with("s_movrel") {
        held.fill(None);
        return;
    }
    match (name, operands) {
        ("s_mov_b32", [Operand::Scalar(destination), source]) => {
            let value = match source {
                Operand::Scalar(from) => held.get(usize::from(*from)).copied().flatten(),
                Operand::Integer(value) => i32::try_from(*value)
                    .ok()
                    .map(|v| DescriptorWord::Constant(u32::from_ne_bytes(v.to_ne_bytes()))),
                Operand::Literal(value) => Some(DescriptorWord::Constant(*value)),
                _ => None,
            };
            if let Some(slot) = held.get_mut(usize::from(*destination)) {
                *slot = value;
            }
        }
        // The compact move of a sixteen-bit immediate, sign-extended, as ACO writes a small
        // constant such as a built descriptor's size.
        ("s_movk_i32", [Operand::Scalar(destination), Operand::Immediate(raw)]) => {
            let value = u16::try_from(*raw).ok().map(|raw| {
                DescriptorWord::Constant(u32::from_ne_bytes(
                    i32::from(i16::from_ne_bytes(raw.to_ne_bytes())).to_ne_bytes(),
                ))
            });
            if let Some(slot) = held.get_mut(usize::from(*destination)) {
                *slot = value;
            }
        }
        ("s_mov_b64", [Operand::Scalar(destination), Operand::Scalar(source)]) => {
            moved_pair(held, *destination, *source);
        }
        (
            _,
            [
                Operand::Scalar(destination),
                Operand::Scalar(base),
                Operand::Immediate(offset),
            ],
        ) if name.starts_with("s_load_dword") => {
            let words = scalar_words(name);
            let table = table_base(held, *base).filter(|_| plain_scalar_offset(instruction));
            let offset = u32::try_from(*offset).ok();
            forget(held, *destination, words);
            if let (Some(table), Some(offset)) = (table, offset) {
                for step in 0..words {
                    if let Some(slot) = held.get_mut(usize::from(*destination) + step as usize) {
                        *slot = Some(DescriptorWord::Loaded {
                            table,
                            offset: offset + step * 4,
                        });
                    }
                }
            }
        }
        _ => {
            let (first, second) = match family {
                // A short vector form's second operand is a source: its carry or compare result,
                // where it has one, goes to `vcc`, which no operand names.
                "SOP1" | "SOP2" | "SOPK" | "VOP1" | "VOP2" | "VOPC" => (true, false),
                "SMEM" => {
                    // A load writes the words its name states, and no more: forgetting past them
                    // lost the pair after a two-word load, a table base the program still read
                    // through (D966). Any other scalar memory instruction forgets four.
                    let load =
                        name.starts_with("s_load_dword") || name.starts_with("s_buffer_load_dword");
                    if let Some(Operand::Scalar(destination)) = operands.first() {
                        let words = scalar_words(name);
                        forget(held, *destination, if load { words } else { words.max(4) });
                    }
                    (false, false)
                }
                "VOP3" => (
                    true,
                    name.contains("_co_")
                        || name.contains("div_scale")
                        || name.contains("mad_u64")
                        || name.contains("mad_i64"),
                ),
                _ => (false, false),
            };
            for (position, writes) in [first, second].into_iter().enumerate() {
                if let (true, Some(Operand::Scalar(register))) = (writes, operands.get(position)) {
                    // A carry out, the second, is a lane mask.
                    let words = if position == 0 {
                        destination_words(name)
                    } else {
                        2
                    };
                    forget(held, *register, words);
                }
            }
            fold_constants(held, instruction, (family, name));
        }
    }
}

#[cfg(test)]
mod table_tests;

#[cfg(test)]
mod tests {
    use super::{BufferSource, DescriptorReads, DescriptorWord, trace};
    use crate::wavefront::{TableBase, TableWord};
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};

    /// Traces a program of words, for a stage whose user data starts at `s0`.
    fn traced(words: &[u32], count: u32) -> super::DrawBuffers {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
        trace(&decoded, &encodings, (0, count), (false, false))
    }

    /// radeonsi's buffer copy as STKT00001 dispatches it, in its least: one dword loaded through
    /// the descriptor in `s[8:11]` and stored through the one in `s[12:15]`, both from user data.
    const COPY: [u32; 6] = [
        0xe030_1000, // buffer_load_dword v1, v0, s[8:11], 0 offen
        0x8002_0100,
        0xbf8c_3f70, // s_waitcnt vmcnt(0)
        0xe070_1000, // buffer_store_dword v1, v0, s[12:15], 0 offen
        0x8003_0100,
        END,
    ];

    /// [`COPY`] with its store an atomic minimum: the second buffer is reached only by an atomic.
    const REDUCE: [u32; 6] = [
        0xe030_1000, // buffer_load_dword v1, v0, s[8:11], 0 offen
        0x8002_0100,
        0xbf8c_3f70, // s_waitcnt vmcnt(0)
        0xe0fc_1000, // buffer_atomic_fmin v1, v0, s[12:15], 0 offen
        0x8003_0100,
        END,
    ];

    /// A buffer reached only by atomics is written, and read too: an atomic folds its value into
    /// the word already there, so the words before the run are what it reduces (D809 is for plain
    /// stores alone). Grand Theft Auto V seeds its scene bounds at +inf and -inf and reduces every
    /// triangle into them with `buffer_atomic_fmin` and `fmax`; started at zero instead, the bounds
    /// it read back were never reduced, every spatial key was zero, and the tree built from them
    /// held cycles that hung the device.
    #[test]
    fn a_buffer_reached_only_by_atomics_is_read() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = REDUCE.iter().flat_map(|w| w.to_le_bytes()).collect();
        let reduce = decode_program(&bytes, &encodings, &operands);
        let names: Vec<String> = reduce
            .instructions
            .iter()
            .map(|i| crate::instruction_name(i, &encodings))
            .collect();
        assert_eq!(
            names,
            [
                "buffer_load_dword",
                "s_waitcnt",
                "buffer_atomic_fmin",
                "s_endpgm"
            ],
            "the hand-assembled reduction"
        );
        let dispatch = trace(&reduce, &encodings, (0, 16), (true, false));
        assert_eq!(
            dispatch.stored,
            std::collections::BTreeSet::from([1]),
            "the atomic's buffer is written"
        );
        assert!(
            dispatch.stored_only.is_empty(),
            "the atomic's buffer is read before it is written, so it is not stored to alone"
        );
    }

    /// A dispatch's image store does not keep its buffers from being traced: the image is bound
    /// whole and written back, as the dispatch's buffers are (D746). Grand Theft Auto V's first
    /// sixteen-wave dispatch reads its constants through a buffer and stores through an image. A
    /// draw's image store, which no snapshot would see, still traces nothing.
    #[test]
    fn a_dispatch_s_image_store_leaves_its_buffers_traced() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let (family, opcode) = encodings.find_by_name("image_store").expect("image_store");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        // image_store v[2:5], v0, s[16:23] dmask:0xf dim:SQ_RSRC_IMG_2D
        let store = [
            encoding.value | (opcode << encoding.opcode.shift) | (0xf << 8) | (1 << 3),
            (2 << 8) | (4 << 16),
        ];
        let mut program = COPY[..3].to_vec();
        program.extend(store);
        program.push(END);
        let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let names: Vec<String> = decoded
            .instructions
            .iter()
            .map(|i| crate::instruction_name(i, &encodings))
            .collect();
        assert_eq!(
            names,
            ["buffer_load_dword", "s_waitcnt", "image_store", "s_endpgm"],
            "the hand-assembled program"
        );
        let dispatch = trace(&decoded, &encodings, (0, 24), (true, false));
        assert_eq!(dispatch.untraced_by, None);
        assert_eq!(dispatch.served, std::collections::BTreeMap::from([(0, 0)]));
        let draw = trace(&decoded, &encodings, (0, 24), (false, false));
        assert_eq!(draw.untraced_by, Some((12, "image_store".to_owned())));
    }

    /// A dispatch that writes with global stores still traces its buffers: each store reaches the
    /// global loads' buffer, which it marks written, as Grand Theft Auto V's compute programs at
    /// 0x74005b854a00 and 0x74005b857200 store through `global_store_dword` beside a
    /// `buffer_load_format_x`. A draw's global store keeps it untraced.
    #[test]
    fn a_dispatch_s_global_store_leaves_its_buffers_traced() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // global_store_dword v[2:3], v1, off.
        let mut program = COPY[..3].to_vec();
        program.extend([0xdc70_8000, 0x007d_0102]);
        program.push(END);
        let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let names: Vec<String> = decoded
            .instructions
            .iter()
            .map(|i| crate::instruction_name(i, &encodings))
            .collect();
        assert_eq!(
            names,
            [
                "buffer_load_dword",
                "s_waitcnt",
                "global_store_dword",
                "s_endpgm"
            ],
            "the hand-assembled program"
        );
        let dispatch = trace(&decoded, &encodings, (0, 24), (true, false));
        assert_eq!(dispatch.untraced_by, None);
        assert_eq!(
            dispatch.served,
            std::collections::BTreeMap::from([(0, 0), (12, 1)])
        );
        let Some(BufferSource::Global(global)) = dispatch.sources.get(1) else {
            panic!(
                "the store reaches the global buffer: {:?}",
                dispatch.sources
            );
        };
        assert_eq!(global.loads, [(12, 4)]);
        assert_eq!(dispatch.stored, std::collections::BTreeSet::from([1]));
        let draw = trace(&decoded, &encodings, (0, 24), (false, false));
        assert_eq!(
            draw.untraced_by,
            Some((12, "global_store_dword".to_owned()))
        );
    }

    /// A computed descriptor after a multiply-add with an inline constant is traced with the program
    /// before it whole: `v_madmk_f32`'s constant is a dword of its own that no literal operand
    /// names, and the prefix is rebuilt from every dword. PPSA03416's fetch shader, past its title
    /// screen, computes so before loading through a descriptor it builds with a scalar offset
    /// (`buffer_load_format_x v4, v4, s[12:15], s11 idxen`).
    #[test]
    fn a_computed_access_after_a_constant_multiply_add_keeps_its_prefix() {
        let madmk = [0x402e_270f, 0x429c_774c]; // v_madmk_f32 v23, v15, 0x429c774c, v19
        let program = [madmk[0], madmk[1], 0xe000_2000, 0x0b03_0404, END];
        let traced = traced(&program, 16);
        let [BufferSource::Computed(computed)] = traced.sources.as_slice() else {
            panic!("one computed buffer: {:?}", traced.sources);
        };
        assert_eq!(computed.prefix, madmk, "the multiply-add's both dwords");
        assert_eq!(computed.at, 8);
    }

    /// A scalar buffer load adding an offset register is found per draw (D753): the descriptor and
    /// the register's value through the program before it, the binding reaching the immediate's
    /// words past the offset. Grand Theft Auto V's pixel shader reads its constants by
    /// `s_buffer_load_dwordx4 s[4:7], s[16:19], 0x0` with `vcc_lo` as the offset register, the
    /// words below as it holds them.
    #[test]
    fn a_scalar_load_at_an_offset_register_is_found_per_draw() {
        let traced = traced(&[0xf428_0108, 0xd400_0000, END], 20);
        let [BufferSource::Computed(computed)] = traced.sources.as_slice() else {
            panic!("one computed access: {:?}", traced.sources);
        };
        assert_eq!(computed.soffset, 106, "vcc_lo");
        let super::ComputedAccess::Descriptor { first, reads } = computed.through else {
            panic!("through a descriptor: {:?}", computed.through);
        };
        assert_eq!((first, reads.scalar_reach), (16, 16));
        assert_eq!(traced.served.get(&0), Some(&0));
    }

    /// A scalar buffer load through a descriptor the program loaded itself is found per draw (D753),
    /// as a vector access through one is: Grand Theft Auto V's pixel shader loads `s[4:7]` from its
    /// constants at `vcc_lo`, then reads `s_buffer_load_dwordx4 s[0:3], s[4:7], 0xb0` through it.
    #[test]
    fn a_scalar_load_through_a_loaded_descriptor_is_found_per_draw() {
        let traced = traced(
            &[0xf428_0108, 0xd400_0000, 0xf428_0002, 0xfa00_00b0, END],
            20,
        );
        let through: Vec<_> = traced
            .sources
            .iter()
            .map(|source| match source {
                BufferSource::Computed(computed) => Some((computed.at, computed.soffset)),
                _ => None,
            })
            .collect();
        assert_eq!(through, [Some((0, 106)), Some((8, 125))]);
        assert_eq!(traced.served.get(&8), Some(&1));
    }

    /// A dispatch's buffer store is traced like its load (D746): the copy reads one buffer and
    /// writes another, each through its own slot. A draw's, a snapshot, traces nothing, as does a
    /// dispatch that writes any other way.
    #[test]
    fn a_dispatch_traces_its_buffer_stores() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let decode = |words: &[u32]| {
            let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            decode_program(&bytes, &encodings, &operands)
        };
        let copy = decode(&COPY);
        let names: Vec<String> = copy
            .instructions
            .iter()
            .map(|i| crate::instruction_name(i, &encodings))
            .collect();
        assert_eq!(
            names,
            [
                "buffer_load_dword",
                "s_waitcnt",
                "buffer_store_dword",
                "s_endpgm"
            ],
            "the hand-assembled copy"
        );
        let dispatch = trace(&copy, &encodings, (0, 16), (true, false));
        assert_eq!(dispatch.sources.len(), 2, "a buffer for each descriptor");
        assert_eq!(
            dispatch.served,
            std::collections::BTreeMap::from([(0, 0), (12, 1)]),
            "the load through slot 0, the store through slot 1"
        );
        assert_eq!(
            dispatch.stored_only,
            std::collections::BTreeSet::from([1]),
            "only the store's buffer is written and never read (D809)"
        );
        assert_eq!(
            dispatch.stored,
            std::collections::BTreeSet::from([1]),
            "the store's buffer is the one written"
        );
        assert!(
            trace(&copy, &encodings, (0, 16), (false, false))
                .sources
                .is_empty(),
            "a draw's store is not traced"
        );
        let mut based = COPY[..3].to_vec();
        // global_store_dword v0, v2, s[0:1]: a store through a scalar base, which no buffer takes.
        // A byte store through a buffer is traced, merged into its word (D977).
        based.extend([0xdc70_8000, 0x0000_0200, END]);
        let other = trace(&decode(&based), &encodings, (0, 16), (true, false));
        assert!(
            other.sources.is_empty(),
            "a dispatch that stores another way traces nothing"
        );
        assert_eq!(
            other.untraced_by,
            Some((12, "global_store_dword".to_owned())),
            "and says which store kept it from tracing"
        );
    }

    /// The open-toolchain GL context's vertex prologue and one four-component attribute fetch
    /// (oops-sdk `glsl_vs.c`, `vs_emit_ngg_preamble` and `vs_load_attributes`): the vertex index
    /// `s12` plus the lane, read from `base + stride * index` with the base and stride from the
    /// table at `s[10:11]`, as three components or four by the table's fourth word.
    const GL_FETCH: [u32; 35] = [
        0xbfa0_0001, // s_inst_prefetch 0x1
        0xbe8d_037e, // s_mov_b32 s13, exec_lo
        0xbefc_03ff,
        0x0000_1003, // s_mov_b32 m0, 0x1003
        0xbf80_0000, // s_nop 0
        0xbf90_0009, // s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbefe_0381, // s_mov_b32 exec_lo, 1
        0x7e02_02ff,
        0x2028_0600, // v_mov_b32 v1, 0x20280600
        0xf800_0941,
        0x0000_0001, // exp prim v1, off, off, off done
        0xbf8c_ff0f, // s_waitcnt expcnt(0)
        0xbefe_0387, // s_mov_b32 exec_lo, 7
        0xd765_000e,
        0x0001_00c1, // v_mbcnt_lo_u32_b32 v14, -1, 0
        0x4a1c_1c0c, // v_add_nc_u32 v14, s12, v14
        0x7e02_0280, // v_mov_b32 v1, 0
        0xf408_0105,
        0xfa00_0000, // s_load_dwordx4 s[4:7], s[10:11], 0x0
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xd569_0004,
        0x0002_1c06, // v_mul_lo_u32 v4, s6, v14
        0xd70f_6a02,
        0x0002_0804, // v_add_co_u32 v2, vcc_lo, s4, v4
        0x5006_0205, // v_add_co_ci_u32_e32 v3, vcc_lo, s5, v1, vcc_lo
        0xbf06_8407, // s_cmp_eq_u32 s7, 4
        0xbf85_0004, // s_cbranch_scc1 4
        0x7e2e_02f2, // v_mov_b32 v23, 1.0
        0xdc3c_8000,
        0x147d_0002, // global_load_dwordx3 v[20:22], v[2:3], off
        0xbf82_0002, // s_branch 2
        0xdc38_8000,
        0x147d_0002, // global_load_dwordx4 v[20:23], v[2:3], off
        0xbf8c_3f70, // s_waitcnt vmcnt(0)
        0xbf81_0000, // s_endpgm
    ];

    /// A primitive shader's global loads with no scalar base share one buffer after the others
    /// (D758): the three- and four-component loads either side of the branch both read it, and
    /// the attribute table the base comes from is a buffer of its own before it. Only a caller
    /// asking for global loads gets them.
    #[test]
    fn a_primitive_shaders_global_loads_share_one_buffer() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = GL_FETCH.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let traced = trace(&decoded, &encodings, (8, 5), (false, true));
        let [BufferSource::Pointer { .. }, BufferSource::Global(global)] =
            traced.sources.as_slice()
        else {
            panic!("the table and the global loads: {:?}", traced.sources);
        };
        assert_eq!(global.loads, vec![(112, 12), (124, 16)]);
        assert_eq!(traced.served.get(&112), Some(&1));
        assert_eq!(traced.served.get(&124), Some(&1));
        let without = trace(&decoded, &encodings, (8, 5), (false, false));
        assert!(
            !without
                .sources
                .iter()
                .any(|source| matches!(source, BufferSource::Global(_)))
        );
    }

    /// A dispatch's global loads with no scalar base are traced as one buffer, as a primitive
    /// shader's are (D883), the sub-word loads among them reading as many bytes as they load: Grand
    /// Theft Auto V's compute program does `global_load_ubyte v0, v[1:2], off`.
    #[test]
    fn a_dispatchs_global_loads_share_one_buffer() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = [0xdc20_0000_u32, 0x007d_0001, END]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let traced = trace(&decoded, &encodings, (0, 2), (true, false));
        let [BufferSource::Global(global)] = traced.sources.as_slice() else {
            panic!("the global load: {:?}", traced.sources);
        };
        assert_eq!(global.loads, vec![(0, 1)]);
        assert_eq!(traced.served.get(&0), Some(&0));
    }

    /// A global load in a loop whose branch back follows it carries the program through that branch,
    /// so the lane run follows every pass: Grand Theft Auto V copies a byte buffer into an image a
    /// pass at a time, its `s_branch` back after its `global_load_ubyte`.
    #[test]
    fn a_looped_global_loads_program_runs_through_its_branch_back() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let (family, opcode) = encodings.find_by_name("v_mov_b32_e32").expect("v_mov");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        let words = [
            // s_mov_b32 s0, 0; loop: v_mov_b32 v1, s0; global_load_dword v3, v[1:2], off;
            // s_add_u32 s0, s0, 4; s_cmp_lt_u32 s0, 12; s_cbranch_scc1 loop; s_endpgm.
            0xbe80_0380,
            encoding.value | (opcode << encoding.opcode.shift) | (1 << 17),
            0xdc30_8000,
            0x037d_0001,
            0x8000_8400,
            0xbf0a_ff00,
            12,
            0xbf85_fff9,
            END,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let traced = trace(&decoded, &encodings, (0, 0), (true, false));
        let [BufferSource::Global(global)] = traced.sources.as_slice() else {
            panic!("the global load: {:?}", traced.sources);
        };
        assert_eq!(global.prefix, words[..8], "through the branch back");
    }

    const END: u32 = 0xbf81_0000;
    /// `s_waitcnt lgkmcnt(0)`.
    const WAIT: u32 = 0xbf8c_c07f;

    /// radeonsi's clear pixel shader: a raw descriptor built from user-data word 2 and three
    /// constants (`si_nir_lower_resource.c:27-51`), read whole by one scalar buffer load.
    const CLEAR: [u32; 10] = [
        0xbe80_0302, // s_mov_b32 s0, s2
        0xbe81_0384, // s_mov_b32 s1, 4
        0xbe82_0390, // s_mov_b32 s2, 16
        0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
        0x3101_6fac,
        0xf428_0000, // s_buffer_load_dwordx4 s[0:3], s[0:3], 0x0
        0xfa00_0000,
        WAIT,
        0x7e00_0200, // v_mov_b32 v0, s0
        END,
    ];

    /// The clear shader's descriptor is traced to user-data word 2 and its three constants, and its
    /// one load reads through it.
    #[test]
    fn the_clear_shaders_descriptor_is_its_user_data_and_constants() {
        let buffers = traced(&CLEAR, 4);
        assert_eq!(
            buffers.sources,
            [BufferSource::Descriptor {
                words: [
                    DescriptorWord::UserData(2),
                    DescriptorWord::Constant(4),
                    DescriptorWord::Constant(16),
                    DescriptorWord::Constant(0x3101_6fac),
                ],
                reads: DescriptorReads {
                    scalar_reach: 16,
                    ..DescriptorReads::default()
                },
            }]
        );
        assert_eq!(buffers.served.get(&0x14), Some(&0));
        assert_eq!(buffers.served.len(), 1);
    }

    /// A descriptor word moved in with `s_movk_i32` is its sixteen-bit immediate sign-extended, as
    /// for the clear shader's `s_mov_b32`.
    #[test]
    fn a_compact_move_is_a_sign_extended_constant() {
        let mut program = CLEAR;
        program[1] = 0xb001_8000; // s_movk_i32 s1, 0x8000
        program[2] = 0xb002_0100; // s_movk_i32 s2, 0x100
        let buffers = traced(&program, 4);
        let [BufferSource::Descriptor { words, .. }] = buffers.sources.as_slice() else {
            panic!("one traced descriptor: {:?}", buffers.sources);
        };
        assert_eq!(
            words[..3],
            [
                DescriptorWord::UserData(2),
                DescriptorWord::Constant(0xffff_8000),
                DescriptorWord::Constant(0x100),
            ]
        );
    }

    /// A descriptor loaded from a table is traced to the table's words, and the load that read it
    /// reads through the table's own buffer, which reaches as far as that load does.
    #[test]
    fn a_descriptor_from_a_table_is_the_tables_words() {
        let program = [
            0xf408_0100, // s_load_dwordx4 s[4:7], s[0:1], 0x10
            0xfa00_0010,
            WAIT,
            0xf420_0202, // s_buffer_load_dword s8, s[4:7], 0x0
            0xfa00_0000,
            END,
        ];
        let buffers = traced(&program, 2);
        let table = TableBase {
            low: TableWord::UserData(0),
            high: TableWord::UserData(1),
        };
        let loaded = |offset| DescriptorWord::Loaded { table, offset };
        assert_eq!(
            buffers.sources,
            [
                BufferSource::Pointer {
                    base: table,
                    extent: 0x20
                },
                BufferSource::Descriptor {
                    words: [loaded(0x10), loaded(0x14), loaded(0x18), loaded(0x1c)],
                    reads: DescriptorReads {
                        scalar_reach: 4,
                        ..DescriptorReads::default()
                    },
                },
            ]
        );
        assert_eq!(buffers.served.get(&0x0), Some(&0));
        assert_eq!(buffers.served.get(&0xc), Some(&1));
    }

    /// A short-form compare reading the scalar register below a descriptor writes `vcc`, not the
    /// registers it reads: the descriptor stays traced. PPSA03416's pixel shader compares against
    /// `s71` between loads through `s[72:75]` (`v_cmp_gt_f32_e32 vcc_lo, s71, v4`).
    #[test]
    fn a_compare_reading_beside_a_descriptor_keeps_it() {
        let program = [
            0xf408_0100, // s_load_dwordx4 s[4:7], s[0:1], 0x10
            0xfa00_0010,
            WAIT,
            0x7c08_0003, // v_cmp_gt_f32_e32 vcc_lo, s3, v0
            0xf420_0202, // s_buffer_load_dword s8, s[4:7], 0x0
            0xfa00_0000,
            END,
        ];
        let buffers = traced(&program, 2);
        assert_eq!(buffers.served.get(&0x10), Some(&1), "{:?}", buffers.served);
    }

    /// A register written with something else on one path to the load is not the user data's: the
    /// value at the load is not one thing, so the load is found per draw (D753), its program
    /// followed where the draw's condition is known.
    #[test]
    fn a_register_written_on_one_path_is_found_per_draw() {
        let program = [
            0xbe80_0302, // s_mov_b32 s0, s2
            0xbe81_0384, // s_mov_b32 s1, 4
            0xbe82_0390, // s_mov_b32 s2, 16
            0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
            0x3101_6fac,
            0xbf85_0001, // s_cbranch_scc0 +1
            0xbe80_0380, // s_mov_b32 s0, 0
            0xf428_0000, // s_buffer_load_dwordx4 s[0:3], s[0:3], 0x0
            0xfa00_0000,
            END,
        ];
        let buffers = traced(&program, 4);
        assert!(
            matches!(buffers.sources.as_slice(), [BufferSource::Computed(_)]),
            "{:?}",
            buffers.sources
        );
    }

    /// Paths that agree on a register keep it traced: a branch around an unrelated write joins with
    /// the descriptor intact.
    #[test]
    fn a_register_both_paths_agree_on_stays_traced() {
        let program = [
            0xbe80_0302, // s_mov_b32 s0, s2
            0xbe81_0384, // s_mov_b32 s1, 4
            0xbe82_0390, // s_mov_b32 s2, 16
            0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
            0x3101_6fac,
            0xbf85_0001, // s_cbranch_scc1 +1
            0xbe8a_0380, // s_mov_b32 s10, 0
            0xf428_0000, // s_buffer_load_dwordx4 s[0:3], s[0:3], 0x0
            0xfa00_0000,
            END,
        ];
        assert_eq!(traced(&program, 4).sources.len(), 1);
    }

    /// A user-data register an arithmetic instruction overwrote is not the draw's word any more:
    /// the load is found per draw (D753), the arithmetic evaluated.
    #[test]
    fn an_overwritten_user_data_register_is_found_per_draw() {
        let program = [
            0x8102_ff02, // s_add_i32 s2, s2, 0x40
            0x0000_0040,
            0xbe80_0302, // s_mov_b32 s0, s2
            0xbe81_0384, // s_mov_b32 s1, 4
            0xbe82_0390, // s_mov_b32 s2, 16
            0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
            0x3101_6fac,
            0xf428_0000, // s_buffer_load_dwordx4 s[0:3], s[0:3], 0x0
            0xfa00_0000,
            END,
        ];
        assert!(matches!(
            traced(&program, 4).sources.as_slice(),
            [BufferSource::Computed(_)]
        ));
    }

    /// Computed loads through one descriptor that nothing between them writes, in one block, share
    /// a buffer, which reaches what each reads (D952). A write to the descriptor between them, or
    /// a block boundary, gives the later load a buffer of its own.
    #[test]
    fn computed_loads_through_one_unwritten_descriptor_share_a_buffer() {
        let add = [0x8100_ff00, 0x0000_0040]; // s_add_i32 s0, s0, 0x40
        let first = [0xf420_0200, 0xfa00_0000]; // s_buffer_load_dword s8, s[0:3], 0x0
        let second = [0xf420_0240, 0xfa00_0004]; // s_buffer_load_dword s9, s[0:3], 0x4
        let program = [
            add[0], add[1], first[0], first[1], second[0], second[1], add[0], add[1], first[0],
            first[1], END,
        ];
        let buffers = traced(&program, 4);
        assert_eq!(
            buffers
                .served
                .iter()
                .map(|(&at, &slot)| (at, slot))
                .collect::<Vec<_>>(),
            [(0x8, 0), (0x10, 0), (0x20, 1)]
        );
        let [BufferSource::Computed(shared), BufferSource::Computed(_)] =
            buffers.sources.as_slice()
        else {
            panic!("two computed buffers: {:?}", buffers.sources);
        };
        assert_eq!(shared.at, 0x8, "found from the first load's prefix");
        let super::ComputedAccess::Descriptor { reads, .. } = shared.through else {
            panic!("through a descriptor");
        };
        assert_eq!(reads.scalar_reach, 8, "the second load's word too");

        // s_cbranch_scc0 +0: the next instruction starts a block.
        let split = [
            add[0],
            add[1],
            first[0],
            first[1],
            0xbf85_0000,
            second[0],
            second[1],
            END,
        ];
        assert_eq!(traced(&split, 4).sources.len(), 2);
    }

    /// A 32-bit scalar write forgets its own register and not the next: `s_add_i32 s3, s3, s3`
    /// leaves the descriptor in `s[4:7]` the user data's, as PPSA02664's vertex shader writes `s7`
    /// by `s_bfe_u32` below a descriptor in `s[8:11]`. A 64-bit one forgets the pair, and the load
    /// is found per draw (D753).
    #[test]
    fn a_scalar_write_forgets_only_its_width() {
        let load = [0xf420_0002, 0xfa00_0000]; // s_buffer_load_dword s0, s[4:7], 0x0
        let program = [0x8103_0303, load[0], load[1], END]; // s_add_i32 s3, s3, s3
        let sources = traced(&program, 8).sources;
        assert!(
            matches!(
                sources.as_slice(),
                [BufferSource::Descriptor { words, .. }]
                    if *words == [4, 5, 6, 7].map(DescriptorWord::UserData)
            ),
            "{sources:?}"
        );
        // s_lshl_b64 s[3:4], s[3:4], 1 writes s4 too.
        let program = [0x8f83_8103, load[0], load[1], END];
        assert!(matches!(
            traced(&program, 8).sources.as_slice(),
            [BufferSource::Computed(_)]
        ));
    }

    /// A program that writes memory is traced not at all: a buffer bound for the draw is its
    /// contents at the draw's start, and a store could change what a later load reads.
    #[test]
    fn a_program_that_stores_is_not_traced() {
        let mut program = CLEAR[..8].to_vec();
        // global_store_dword v[0:1], v2, off
        program.extend([0xdc70_8000, 0x007d_0200, END]);
        assert!(traced(&program, 4).sources.is_empty());
    }

    /// The clear shader's pixel module, translated for a draw that binds its buffers, declares
    /// them at the pixel stage's binding of the draw-buffer set and reads them there; translated
    /// for one that does not, it declares nothing there and reads the window as before.
    #[test]
    fn a_draw_that_binds_buffers_reads_the_clear_colour_through_its_binding() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData, Window};
        use crate::{Fidelity, Strategy, Width, translate_with_user_data};
        use orbistoun_spirv::{DRAW_BUFFERS_SET, PIXEL_BUFFERS_BINDING, decoration, op};

        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = CLEAR.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let module = |draw_buffers: bool| {
            translate_with_user_data(
                &decoded,
                &encodings,
                Strategy::Predicated {
                    fidelity: Fidelity::Wavefront,
                    width: Width::Wave64,
                },
                (Stage::Fragment, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    count: 4,
                    block_offset: 16,
                    draw_buffers,
                    ..UserData::default()
                },
            )
            .expect("the clear shader translates")
            .module
        };
        // Every instruction of a module, as its opcode and operand words.
        let instructions = |words: &[u32]| -> Vec<(u16, Vec<u32>)> {
            let mut at = 5;
            let mut found = Vec::new();
            while let Some(&word) = words.get(at) {
                let count = (word >> 16) as usize;
                found.push(((word & 0xffff) as u16, words[at + 1..at + count].to_vec()));
                at += count;
            }
            found
        };
        let decorated = |words: &[u32], what: u32, value: u32| {
            instructions(words).iter().any(|(opcode, operands)| {
                *opcode == op::DECORATE && operands.get(1..) == Some(&[what, value][..])
            })
        };
        let lengths = |words: &[u32]| {
            instructions(words)
                .iter()
                .filter(|(opcode, _)| *opcode == op::ARRAY_LENGTH)
                .count()
        };
        let bound = module(true);
        assert!(decorated(
            &bound,
            decoration::DESCRIPTOR_SET,
            DRAW_BUFFERS_SET
        ));
        assert!(decorated(
            &bound,
            decoration::BINDING,
            PIXEL_BUFFERS_BINDING
        ));
        assert_eq!(lengths(&bound), 4, "one read per word of the colour");
        let windowed = module(false);
        assert!(!decorated(
            &windowed,
            decoration::DESCRIPTOR_SET,
            DRAW_BUFFERS_SET
        ));
        assert_eq!(lengths(&windowed), 0);
    }

    /// Two loads through one descriptor read one buffer.
    #[test]
    fn two_loads_through_one_descriptor_share_a_slot() {
        let program = [
            0xbe80_0302, // s_mov_b32 s0, s2
            0xbe81_0384, // s_mov_b32 s1, 4
            0xbe82_0390, // s_mov_b32 s2, 16
            0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
            0x3101_6fac,
            0xf420_0200, // s_buffer_load_dword s8, s[0:3], 0x0
            0xfa00_0000,
            0xf420_0240, // s_buffer_load_dword s9, s[0:3], 0x4
            0xfa00_0004,
            END,
        ];
        let buffers = traced(&program, 4);
        assert_eq!(buffers.sources.len(), 1);
        assert_eq!(buffers.served.values().copied().collect::<Vec<_>>(), [0, 0]);
        let BufferSource::Descriptor { reads, .. } = buffers.sources[0] else {
            panic!("a descriptor: {:?}", buffers.sources[0]);
        };
        assert_eq!(reads.scalar_reach, 8, "the second load reaches furthest");
    }

    /// The descriptor a vertex fetch indexes, in user data: an indexed access at an immediate
    /// offset reaches that offset and its width into each record.
    #[test]
    fn an_indexed_fetch_reaches_its_offset_and_width_into_a_record() {
        // buffer_load_dword v1, v0, s[0:3], 0 idxen offset:8
        let program = [0xe030_2008, 0x8000_0100, END];
        let buffers = traced(&program, 4);
        assert_eq!(
            buffers.sources,
            [BufferSource::Descriptor {
                words: [0, 1, 2, 3].map(DescriptorWord::UserData),
                reads: DescriptorReads {
                    scalar_reach: 0,
                    vector: true,
                    vector_reach: Some(12),
                    indexed: true,
                    index: None,
                },
            }]
        );
    }

    /// Traces a primitive shader's program of words, its twenty user-data words from `s8`, as
    /// MCUB00001's (D879).
    fn traced_primitive(words: &[u32], globals: bool) -> super::DrawBuffers {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
        trace(&decoded, &encodings, (8, 20), (false, globals))
    }

    /// What each traced descriptor's indexed accesses index by, in slot order.
    fn indices(buffers: &super::DrawBuffers) -> Vec<Option<super::RecordIndex>> {
        buffers
            .sources
            .iter()
            .map(|source| match source {
                BufferSource::Descriptor { reads, .. } => reads.index,
                other => panic!("a descriptor: {other:?}"),
            })
            .collect()
    }

    /// `v_add_nc_u32 v5, s13, v5`: the vertex id plus the base vertex, user-data word 5.
    const ADD_BASE_VERTEX: u32 = 0x4a0a_0a0d;
    /// `buffer_load_format_xyzw v[0:3], v5, s[20:23], 0 idxen`.
    const FETCH_POSITION: [u32; 2] = [0xe00c_2000, 0x8005_0005];
    /// `buffer_load_format_xyzw v[4:7], v5, s[24:27], 0 idxen`.
    const FETCH_COLOUR: [u32; 2] = [0xe00c_2000, 0x8006_0405];

    /// MCUB00001's (Mesa Cube) vertex fetch: both attributes indexed by the vertex id plus the
    /// base vertex in user data, so each draw reaches only the records its vertices name (D879).
    /// The first fetch's four registers do not reach `v5`, which the second reads.
    #[test]
    fn a_fetch_by_the_vertex_id_plus_the_base_vertex_says_so() {
        let mut program = vec![ADD_BASE_VERTEX];
        program.extend(FETCH_POSITION);
        program.extend(FETCH_COLOUR);
        program.push(END);
        let buffers = traced_primitive(&program, true);
        let by = Some(super::RecordIndex::Vertex(Some(DescriptorWord::UserData(
            5,
        ))));
        assert_eq!(indices(&buffers), [by, by]);
        let BufferSource::Descriptor { words, .. } = buffers.sources[0] else {
            unreachable!()
        };
        assert_eq!(words, [12, 13, 14, 15].map(DescriptorWord::UserData));
    }

    /// The ids as the module seeds them: `v5` the vertex, `v8` the instance, each followed through
    /// a move. A fetch through `v0`, which holds the primitive's vertex indices, says nothing.
    #[test]
    fn the_seeded_ids_are_followed_through_a_move() {
        let program = [
            0x7e02_0305, // v_mov_b32 v1, v5
            0xe00c_2000, // buffer_load_format_xyzw v[2:5], v1, s[20:23], 0 idxen
            0x8005_0201,
            0xe00c_2000, // buffer_load_format_xyzw v[0:3], v8, s[24:27], 0 idxen
            0x8006_0008,
            0xe00c_2000, // buffer_load_format_xyzw v[0:3], v0, s[16:19], 0 idxen
            0x8004_0000,
            END,
        ];
        let buffers = traced_primitive(&program, true);
        assert_eq!(
            indices(&buffers),
            [
                Some(super::RecordIndex::Vertex(None)),
                Some(super::RecordIndex::Instance(None)),
                None
            ]
        );
    }

    /// A write to the register holding an id leaves it holding none: here the colour fetch's own
    /// destination, `v[4:7]`, covers `v5` before the next fetch reads it.
    #[test]
    fn an_id_overwritten_bounds_nothing() {
        let mut program = vec![ADD_BASE_VERTEX];
        program.extend(FETCH_COLOUR);
        program.extend(FETCH_POSITION);
        program.push(END);
        let buffers = traced_primitive(&program, true);
        let by = Some(super::RecordIndex::Vertex(Some(DescriptorWord::UserData(
            5,
        ))));
        assert_eq!(indices(&buffers), [by, None]);
    }

    /// Only a primitive shader's `v5` and `v8` are ids: a fragment shader's or a dispatch's fetch
    /// by them says nothing.
    #[test]
    fn only_a_primitive_shader_seeds_ids() {
        let mut program = vec![ADD_BASE_VERTEX];
        program.extend(FETCH_POSITION);
        program.push(END);
        assert_eq!(indices(&traced_primitive(&program, false)), [None]);
    }

    /// Two fetches through one descriptor bound it only when they index by the same thing: the
    /// vertex id and the primitive's indices in `v0` do not agree.
    #[test]
    fn fetches_through_one_descriptor_must_agree() {
        let mut program = vec![ADD_BASE_VERTEX];
        program.extend(FETCH_POSITION);
        // buffer_load_format_xyzw v[8:11], v0, s[20:23], 0 idxen
        program.extend([0xe00c_2000, 0x8005_0800]);
        program.push(END);
        assert_eq!(indices(&traced_primitive(&program, true)), [None]);
        // The same fetch twice agrees.
        let mut program = vec![ADD_BASE_VERTEX];
        program.extend(FETCH_POSITION);
        program.extend([0xe00c_2000, 0x8005_0805]);
        program.push(END);
        assert_eq!(
            indices(&traced_primitive(&program, true)),
            [Some(super::RecordIndex::Vertex(Some(
                DescriptorWord::UserData(5)
            )))]
        );
    }

    /// A format access moves at most one thirty-two-bit component per channel it names, so its reach
    /// into a record is four bytes a channel: the AGC formatted copy's `buffer_store_format_x`
    /// (PPSA03416) reaches four, and its last record ends at the buffer's end.
    #[test]
    fn a_format_access_reaches_four_bytes_a_channel() {
        // buffer_store_format_x v1, v0, s[4:7], 0 idxen; buffer_load_format_xyzw v[1:4], v0,
        // s[0:3], 0 idxen
        for (program, reach) in [
            ([0xe010_2000, 0x8001_0100, END], 4),
            ([0xe00c_2000, 0x8000_0100, END], 16),
        ] {
            let encodings = EncodingTable::builtin().expect("encodings");
            let operands = OperandTable::builtin().expect("operands");
            let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
            let decoded = decode_program(&bytes, &encodings, &operands);
            // A dispatch's stores are traced (D746).
            let buffers = trace(&decoded, &encodings, (0, 8), (true, false));
            let [BufferSource::Descriptor { reads, .. }] = buffers.sources[..] else {
                panic!("one descriptor: {:?}", buffers.sources);
            };
            assert_eq!(reads.vector_reach, Some(reach), "{program:x?}");
        }
    }

    /// A scalar load adding a scalar offset register is found per draw too (D753), as PPSA28061's
    /// vertex shader loads its descriptor: the base pair's register, how far the load reaches past
    /// base and offset, and the offset's code.
    #[test]
    fn a_scalar_load_adding_a_scalar_offset_is_computed_per_draw() {
        // s_load_dwordx4 s[0:3], s[8:9], vcc_hi
        let program = [0xf408_0004, 0xd600_0000, END];
        let buffers = traced(&program, 12);
        let [BufferSource::Computed(computed)] = buffers.sources.as_slice() else {
            panic!("one computed source: {:?}", buffers.sources);
        };
        assert_eq!(
            (computed.through, computed.soffset),
            (super::ComputedAccess::Pointer { base: 8, end: 16 }, 0x6b)
        );
        assert_eq!(buffers.served.get(&0), Some(&0));
    }

    /// A fetch that adds a scalar offset register is found per draw (D753): the range check does
    /// not see that offset, so the descriptor alone does not bound where it reads. Its source is
    /// the program before it, the descriptor's registers and the offset's code.
    #[test]
    fn a_fetch_adding_a_scalar_offset_is_computed_per_draw() {
        // s_nop 0; buffer_load_dword v1, v0, s[0:3], s5 idxen offset:8
        let program = [0xbf80_0000, 0xe030_2008, 0x0500_0100, END];
        let buffers = traced(&program, 6);
        let [BufferSource::Computed(computed)] = buffers.sources.as_slice() else {
            panic!("one computed source: {:?}", buffers.sources);
        };
        assert!(matches!(
            computed.through,
            super::ComputedAccess::Descriptor { first: 0, .. }
        ));
        assert_eq!(
            (computed.prefix.as_slice(), computed.at, computed.soffset),
            (&[0xbf80_0000][..], 4, 5)
        );
        assert_eq!((computed.first_register, computed.user_data), (0, 6));
        assert_eq!(buffers.served.get(&4), Some(&0));
    }

    /// Each ray intersection's hierarchy descriptor is traced as a buffer descriptor is: from user
    /// data, from a table at a user-data base, or - once the program changes a word of it - computed
    /// per dispatch from the program before it (D753). One descriptor read twice is one source.
    #[test]
    fn a_ray_intersection_s_descriptor_is_traced() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let program = [
            0xf408_0100, // s_load_dwordx4 s[4:7], s[0:1], 0x10
            0xfa00_0010,
            WAIT,
            0xf198_9f01, // image_bvh_intersect_ray v[4:7], v[0:10], s[8:11]
            0x0002_0400,
            0xf198_9f01, // image_bvh_intersect_ray v[4:7], v[0:10], s[4:7]
            0x0001_0400,
            0xf198_9f01, // image_bvh_intersect_ray v[4:7], v[0:10], s[8:11]
            0x0002_0400,
            0x8004_0c04, // s_add_u32 s4, s4, s12
            0xf198_9f01, // image_bvh_intersect_ray v[4:7], v[0:10], s[4:7]
            0x0001_0400,
            END,
        ];
        let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
        let sources = super::trace_hierarchies(&decoded, &encodings, (0, 16));
        let table = TableBase {
            low: TableWord::UserData(0),
            high: TableWord::UserData(1),
        };
        let loaded = |offset| DescriptorWord::Loaded { table, offset };
        let [
            super::HierarchySource::Descriptor(from_user_data),
            super::HierarchySource::Descriptor(from_table),
            super::HierarchySource::Computed(computed),
        ] = sources.as_slice()
        else {
            panic!("two traced descriptors and a computed one: {sources:?}");
        };
        assert_eq!(
            *from_user_data,
            [8, 9, 10, 11].map(DescriptorWord::UserData)
        );
        assert_eq!(
            *from_table,
            [loaded(0x10), loaded(0x14), loaded(0x18), loaded(0x1c)]
        );
        assert_eq!(
            (computed.at, computed.through, computed.soffset),
            (
                40,
                super::ComputedAccess::Descriptor {
                    first: 4,
                    reads: DescriptorReads::default()
                },
                125
            )
        );
        assert_eq!(computed.prefix, program[..10]);
    }

    /// A flat store whose address's high word a `v_med3_u32` clamps between the private and the
    /// local data share's apertures reaches no guest memory (D989), as Grand Theft Auto V's ray
    /// traversal pushes its stack at 0x74005b854a00 + 0x12d0. With the clamp's bound unknown, or the
    /// mask changed after it, the store is not known to stay off guest memory.
    #[test]
    fn a_flat_store_clamped_to_the_apertures_reaches_no_guest_memory() {
        const BREV: u32 = 0xbee5_0b81; // s_brev_b32 s101, 1
        // v_med3_u32 v9, s101, 0x70000000, v43
        const MED3: [u32; 3] = [0xd559_0009, 0x04ad_fe65, 0x7000_0000];
        const STORE: [u32; 2] = [0xdc70_0000, 0x007d_0308]; // flat_store_dword v[8:9], v3
        const CMPX: u32 = 0x7daa_0283; // v_cmpx_ne_u32_e32 3, v1
        let beyond = |words: &[u32]| {
            let encodings = EncodingTable::builtin().expect("encodings");
            let operands = OperandTable::builtin().expect("operands");
            let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            let decoded = decode_program(&bytes, &encodings, &operands);
            assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
            super::flat_stores_beyond_guest(&decoded, &encodings, (0, 4))
        };
        let clamped = [&[BREV][..], &MED3, &STORE, &[END]].concat();
        assert_eq!(beyond(&clamped).into_iter().collect::<Vec<_>>(), [16]);
        // s101 never set: the clamp's floor is not known.
        let unknown = [&MED3[..], &STORE, &[END]].concat();
        assert!(beyond(&unknown).is_empty());
        // The mask changed between the clamp and the store: a lane left out of the clamp keeps
        // whatever its register held.
        let masked = [&[BREV][..], &MED3, &[CMPX], &STORE, &[END]].concat();
        assert!(beyond(&masked).is_empty());
        // The same store in the global segment (`SEG` 2) is not a flat one.
        let global = [&[BREV][..], &MED3, &[STORE[0] | 2 << 14, STORE[1]], &[END]].concat();
        assert!(beyond(&global).is_empty());
    }
}
