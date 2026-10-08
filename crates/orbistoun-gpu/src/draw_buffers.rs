//! A draw's buffers resolved from guest memory (D733).
//!
//! The translator names each buffer a module reads through by where its descriptor or base comes
//! from ([`BufferSource`]); per draw, this finds the guest range each names - from the draw's user
//! data, constants and the descriptor-table words it reads - and the bytes the descriptor's bounds
//! rules let the module reach, and hands them to the backend as a
//! [`RenderCommand::BindDrawBuffers`]. A range that cannot be bound exactly is refused by name.

use std::collections::BTreeMap;

use orbistoun_translate::draw_buffers::{
    BufferSource, Computed, ComputedAccess, DescriptorReads, DescriptorWord, GlobalLoads,
};
use orbistoun_translate::wavefront::{TableBase, TableWord};

use crate::backend::{
    DrawBuffer, IndexBuffer, RenderCommand, ResourceId, ShaderStage, USER_DATA_WORDS,
};
use crate::pipeline::GuestMemory;
use crate::registers::decode_buffer_descriptor;

/// The bytes from a descriptor's base every read the program makes through it can reach, as the
/// descriptor's bounds rules admit them, or why nothing bounds them.
///
/// # Errors
///
/// A refusal naming what leaves the reads unbounded.
pub fn descriptor_extent(words: [u32; 4], reads: DescriptorReads) -> Result<u64, &'static str> {
    const UNBOUNDED: &str = concat!(
        "a buffer read at a register offset or record index its descriptor's out-of-bounds mode ",
        "does not limit, so no range holds every read (D733)"
    );
    let descriptor = decode_buffer_descriptor(words);
    if descriptor.unsupported {
        return Err("a buffer descriptor asking for swizzled or thread-indexed addressing (D733)");
    }
    let (stride, records) = (u64::from(descriptor.stride), u64::from(descriptor.records));
    // A scalar buffer load reads each word within the record count, which is in bytes with no
    // stride and in strides with one (the translator's `scalar_buffer_load`; radeonsi
    // `si_state.c:3506-3527`).
    let scalar = if stride == 0 {
        records
    } else {
        records * stride
    }
    .min(u64::from(reads.scalar_reach));
    if !reads.vector {
        return Ok(scalar);
    }
    let reach = reads.vector_reach.map(u64::from);
    // Where the last record an access can index starts: every record's when it indexes, the
    // first's when not.
    let last_record = if reads.indexed {
        records.saturating_sub(1) * stride
    } else {
        0
    };
    // The four out-of-bounds modes (`OOB_SELECT`, bits 29:28 of the fourth word, Mesa
    // `gfx10-rsrc.json`), as the translator's `buffer_out_of_bounds` applies them. An access reads
    // at most sixteen bytes from its offset.
    let vector = match (words[3] >> 28) & 3 {
        // Structured with offset: the index below the record count and the offset below the stride.
        0 if records == 0 || stride == 0 => 0,
        0 => last_record + reach.unwrap_or(u64::MAX).min(stride - 1 + 16),
        // Structured: the index below the record count, the offset unchecked.
        1 if records == 0 => 0,
        1 => last_record + reach.ok_or(UNBOUNDED)?,
        // Raw: the offset and the access's width within the record count in bytes, the index
        // unchecked.
        3 if reads.indexed && stride != 0 => return Err(UNBOUNDED),
        3 => reach.map_or(records, |reach| reach.min(records)),
        // Only a record count of zero is out of bounds.
        _ if records == 0 => 0,
        _ if reads.indexed && stride != 0 => return Err(UNBOUNDED),
        _ => reach.ok_or(UNBOUNDED)?,
    };
    Ok(scalar.max(vector))
}

/// The most bytes one draw buffer binds; a descriptor naming more is refused rather than read.
const MOST_BYTES: u64 = 256 << 20;

/// How many ranges [`BufferCache`] keeps before it restarts: a frame's constants and vertices, and
/// a bound for a guest streaming new ones every frame.
const CACHE_ENTRIES: usize = 256;

/// Guest bytes read for draws, by where they lie and how many, kept across submissions while the
/// host's write tracking reports them unwritten, as texels are.
#[derive(Debug, Default)]
pub struct BufferCache {
    entries: std::collections::HashMap<(u64, usize), (Option<u64>, DrawBuffer)>,
}

impl BufferCache {
    /// The `length` bytes at `address`, from the cache while nothing has written them.
    fn read(
        &mut self,
        address: u64,
        length: usize,
        memory: &impl GuestMemory,
    ) -> Result<DrawBuffer, &'static str> {
        // Nothing to read: a buffer no lane reaches (D758).
        if length == 0 {
            return Ok(DrawBuffer {
                hash: crate::ContentHasher::new(0).finish(),
                bytes: Vec::new().into(),
                base: address,
            });
        }
        let span = length as u64;
        if let Some((since, buffer)) = self.entries.get(&(address, length))
            && since.and_then(|since| orbistoun_mem::watch::written_since(address, span, since))
                == Some(false)
        {
            return Ok(buffer.clone());
        }
        // Marked before the bytes are hashed, so a write racing the hash is seen next time.
        let since = orbistoun_mem::watch::mark(address, span);
        let bytes = memory.read(address, length).ok_or(concat!(
            "a draw buffer's range is not wholly mapped guest memory, so the bytes its reads ",
            "reach are not there to bind (D733)"
        ))?;
        // Whole words, the tail past the range zero: the module never reads a word the range
        // does not start.
        let mut padded = bytes.to_vec();
        padded.resize(length.div_ceil(4) * 4, 0);
        let mut hasher = crate::ContentHasher::new(padded.len() / 4);
        hasher.bytes(&padded);
        let buffer = DrawBuffer {
            hash: hasher.finish(),
            bytes: padded.into(),
            base: address,
        };
        if self.entries.len() >= CACHE_ENTRIES {
            self.entries.clear();
        }
        self.entries
            .insert((address, length), (since, buffer.clone()));
        Ok(buffer)
    }
}

/// The value one descriptor word or address half has for a draw with these user-data words.
pub(crate) fn resolve_word(
    word: DescriptorWord,
    user_data: &[u32; USER_DATA_WORDS],
    memory: &impl GuestMemory,
) -> Result<u32, &'static str> {
    match word {
        DescriptorWord::UserData(index) => usize::try_from(index)
            .ok()
            .and_then(|index| user_data.get(index).copied())
            .ok_or("a buffer descriptor word from a user-data word the stage does not have"),
        DescriptorWord::Constant(value) => Ok(value),
        DescriptorWord::Loaded { table, offset } => {
            let at = table_address(table, user_data).wrapping_add(u64::from(offset));
            let bytes = memory
                .read(at, 4)
                .ok_or("a buffer descriptor read from a table that is not mapped (D733)")?;
            Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
    }
}

/// What a computed access reads through, and its scalar offset, for a draw with these user-data
/// words (D753): the program before the access, run over them and guest memory. A descriptor's four
/// words, or a scalar load's base pair in the first two.
///
/// # Errors
///
/// A refusal naming why the prefix leaves what it reads through or the offset unknown.
pub(crate) fn evaluate_computed(
    computed: &Computed,
    user_data: &[u32; USER_DATA_WORDS],
    memory: &impl GuestMemory,
) -> Result<([u32; 4], u32), &'static str> {
    const UNKNOWN: &str = concat!(
        "a buffer descriptor the program computes from something the host cannot know for the ",
        "draw - a vector result, the execution mask, or an operation not evaluated (D753)"
    );
    let (encodings, decode) = decode_prefix(&computed.prefix)?;
    let count = usize::try_from(computed.user_data)
        .unwrap_or(USER_DATA_WORDS)
        .min(USER_DATA_WORDS);
    let scalars = orbistoun_translate::evaluate::scalar_prefix(
        &decode,
        encodings,
        computed.at,
        (computed.first_register, &user_data[..count]),
        &mut |address| {
            let bytes = memory.read(address, 4)?;
            Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        },
    )?;
    let (first, count) = match computed.through {
        ComputedAccess::Descriptor { first, .. } => (usize::from(first), 4),
        ComputedAccess::Pointer { base, .. } => (usize::from(base), 2),
    };
    let mut words = [0u32; 4];
    for (step, word) in words.iter_mut().enumerate().take(count) {
        *word = scalars
            .get(first + step)
            .copied()
            .flatten()
            .ok_or(UNKNOWN)?;
    }
    // The offset field's codes: a scalar register, `null` (125), or an inline integer, 128 to 192
    // counting up from zero and 193 to 208 down from -1.
    let soffset = match computed.soffset {
        125 => 0,
        code @ 128..=192 => u32::from(code - 128),
        code @ 193..=208 => 0u32.wrapping_sub(u32::from(code - 192)),
        code => scalars
            .get(usize::from(code))
            .copied()
            .flatten()
            .ok_or(UNKNOWN)?,
    };
    Ok((words, soffset))
}

/// The shader tables, and a prefix of a program decoded with them.
fn decode_prefix(
    prefix: &[u32],
) -> Result<
    (
        &'static orbistoun_shader::EncodingTable,
        orbistoun_shader::Decode,
    ),
    &'static str,
> {
    static TABLES: std::sync::OnceLock<
        Option<(
            orbistoun_shader::EncodingTable,
            orbistoun_shader::OperandTable,
        )>,
    > = std::sync::OnceLock::new();
    let (encodings, operands) = TABLES
        .get_or_init(|| {
            Some((
                orbistoun_shader::EncodingTable::builtin().ok()?,
                orbistoun_shader::OperandTable::builtin().ok()?,
            ))
        })
        .as_ref()
        .ok_or("the shader encoding tables did not load")?;
    let bytes: Vec<u8> = prefix.iter().flat_map(|word| word.to_le_bytes()).collect();
    Ok((
        encodings,
        orbistoun_shader::decode_program(&bytes, encodings, operands),
    ))
}

/// The guest range a primitive shader's global loads read for a draw of `vertices` vertices with
/// these user-data words (D758): from the lowest address an active lane forms to the end of the
/// widest read from the highest. Empty, at zero, when no lane reaches a load.
///
/// # Errors
///
/// A refusal naming why the lanes' addresses cannot be found.
fn global_range(
    global: &GlobalLoads,
    user_data: &[u32; USER_DATA_WORDS],
    vertices: u32,
    memory: &impl GuestMemory,
) -> Result<(u64, u64), &'static str> {
    /// A run's answer, and every word it read with the value it read: the answer holds while they
    /// all still read the same.
    type Run = (Vec<(u64, Option<u32>)>, Result<(u64, u64), &'static str>);
    /// Runs kept, by program, user data and vertices; a bound, since a guest streaming new tables
    /// every frame adds a run a draw.
    const RUNS_KEPT: usize = 4096;
    static RUNS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<Vec<u32>, Run>>> =
        std::sync::OnceLock::new();
    if vertices > global.width {
        return Err(concat!(
            "a draw of more vertices than one wave holds, whose global loads' lanes are not all ",
            "run by one (D758)"
        ));
    }
    let count = usize::try_from(global.user_data)
        .unwrap_or(USER_DATA_WORDS)
        .min(USER_DATA_WORDS);
    let word = |address: u64| {
        let bytes = memory.read(address, 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    // The same program over the same user data and vertices reads the same words; while they hold
    // what they held, the lanes form the same addresses.
    let hash = crate::content_hash(&global.prefix);
    let mut key = vec![
        hash as u32,
        (hash >> 32) as u32,
        global.first_register,
        global.width,
        vertices,
    ];
    key.extend_from_slice(&user_data[..count]);
    let runs = RUNS.get_or_init(Default::default);
    if let Ok(runs) = runs.lock()
        && let Some((reads, answer)) = runs.get(&key)
        && reads.iter().all(|&(address, value)| word(address) == value)
    {
        return *answer;
    }
    let mut reads = Vec::new();
    let answer = run_global_loads(global, (&user_data[..count], vertices), &mut |address| {
        let value = word(address);
        reads.push((address, value));
        value
    });
    if let Ok(mut runs) = runs.lock() {
        if runs.len() >= RUNS_KEPT {
            runs.clear();
        }
        runs.insert(key, (reads, answer));
    }
    answer
}

/// [`global_range`] found by running the program, reading guest memory through `read`.
fn run_global_loads(
    global: &GlobalLoads,
    (user_data, vertices): (&[u32], u32),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Result<(u64, u64), &'static str> {
    use orbistoun_translate::evaluate::LaneProgram;
    /// Programs prepared for runs, by their words: each is decoded and sliced once.
    type Programs = std::collections::HashMap<Vec<u32>, std::sync::Arc<LaneProgram>>;
    static PROGRAMS: std::sync::OnceLock<std::sync::Mutex<Programs>> = std::sync::OnceLock::new();
    let (encodings, _) = decode_prefix(&[])?;
    let programs = PROGRAMS.get_or_init(Default::default);
    let known = programs
        .lock()
        .ok()
        .and_then(|programs| programs.get(&global.prefix).cloned());
    let program = if let Some(program) = known {
        program
    } else {
        let (_, decode) = decode_prefix(&global.prefix)?;
        let offsets: Vec<u32> = global.loads.iter().map(|&(at, _)| at).collect();
        let program = std::sync::Arc::new(LaneProgram::new(decode, encodings, &offsets));
        if let Ok(mut programs) = programs.lock() {
            programs.insert(global.prefix.clone(), program.clone());
        }
        program
    };
    let found = orbistoun_translate::evaluate::lane_addresses(
        &program,
        encodings,
        (global.first_register, user_data),
        (global.width, vertices),
        read,
    )?;
    let mut span: Option<(u64, u64)> = None;
    for (at, addresses) in found {
        let reads = global
            .loads
            .iter()
            .find(|&&(load, _)| load == at)
            .map_or(16, |&(_, bytes)| u64::from(bytes));
        for address in addresses {
            let end = address.saturating_add(reads);
            span = Some(span.map_or((address, end), |(low, high)| {
                (low.min(address), high.max(end))
            }));
        }
    }
    // Whole words, from the word the lowest address is in, as the module reads them.
    Ok(span.map_or((0, 0), |(low, high)| (low & !3, high - (low & !3))))
}

/// A buffer descriptor's fourth word for a draw with these user-data words - its format and
/// selects (D738) - where the source has one and it can be found.
pub(crate) fn resolve_fourth_word(
    source: &BufferSource,
    user_data: &[u32; USER_DATA_WORDS],
    memory: &impl GuestMemory,
) -> Option<u32> {
    match source {
        BufferSource::Descriptor { words, .. } => resolve_word(words[3], user_data, memory).ok(),
        BufferSource::Computed(computed)
            if matches!(computed.through, ComputedAccess::Descriptor { .. }) =>
        {
            evaluate_computed(computed, user_data, memory)
                .ok()
                .map(|(words, _)| words[3])
        }
        // A scalar load's base names no format, and a global load has no descriptor.
        BufferSource::Computed(_) | BufferSource::Pointer { .. } | BufferSource::Global(_) => None,
    }
}

/// A table or scalar-load base's 64-bit address for a draw with these user-data words.
fn table_address(table: TableBase, user_data: &[u32; USER_DATA_WORDS]) -> u64 {
    let half = |word: TableWord| match word {
        TableWord::UserData(index) => usize::try_from(index)
            .ok()
            .and_then(|index| user_data.get(index).copied())
            .unwrap_or(0),
        TableWord::Constant(value) => value,
    };
    u64::from(half(table.low)) | u64::from(half(table.high)) << 32
}

/// The guest range a source names for a draw of `vertices` vertices with these user-data words: its
/// address and length.
///
/// # Errors
///
/// A refusal naming why the range cannot be bound exactly.
pub(crate) fn resolve_range(
    source: &BufferSource,
    (user_data, vertices): (&[u32; USER_DATA_WORDS], u32),
    memory: &impl GuestMemory,
) -> Result<(u64, u64), &'static str> {
    let (address, length) = match source {
        BufferSource::Global(global) => global_range(global, user_data, vertices, memory)?,
        BufferSource::Pointer { base, extent } => {
            (table_address(*base, user_data), u64::from(*extent))
        }
        // From the descriptor's base, through the scalar offset the shader adds to every address
        // and the extent the descriptor admits past it.
        BufferSource::Computed(computed) => {
            let (values, soffset) = evaluate_computed(computed, user_data, memory)?;
            match computed.through {
                ComputedAccess::Descriptor { reads, .. } => (
                    crate::pipeline::guest_address_of(decode_buffer_descriptor(values).base),
                    u64::from(soffset) + descriptor_extent(values, reads)?,
                ),
                // A scalar load reads its words from the base plus the scalar offset, which the
                // translated load does not add: the range starts there.
                ComputedAccess::Pointer { end, .. } => {
                    let base = u64::from(values[0]) | u64::from(values[1] & 0xffff) << 32;
                    (base.wrapping_add(u64::from(soffset)), u64::from(end))
                }
            }
        }
        BufferSource::Descriptor { words, reads } => {
            let (words, reads) = (*words, *reads);
            let mut values = [0u32; 4];
            for (value, word) in values.iter_mut().zip(words) {
                *value = resolve_word(word, user_data, memory)?;
            }
            let base = decode_buffer_descriptor(values).base;
            (
                crate::pipeline::guest_address_of(base),
                descriptor_extent(values, reads)?,
            )
        }
    };
    // The module reads whole words counted from the range's first byte, as the hardware does from
    // a word-aligned base.
    if address % 4 != 0 {
        return Err("a draw buffer whose base is not word-aligned (D733)");
    }
    if length > MOST_BYTES {
        return Err("a draw buffer larger than one draw binds (D733)");
    }
    Ok((address, length))
}

/// The vertex ids a chunk of a non-indexed draw reads in place of indices (D741): `count`
/// thirty-two-bit ids counting up from its first; empty for anything else.
fn counting_indices(indices: Option<Result<(IndexBuffer, u32), &'static str>>) -> DrawBuffer {
    let ids: Vec<u32> = match indices {
        Some(Ok((IndexBuffer::Counting { first }, count))) => {
            (0..count).map(|id| first.wrapping_add(id)).collect()
        }
        _ => Vec::new(),
    };
    let bytes: Vec<u8> = ids.iter().flat_map(|id| id.to_le_bytes()).collect();
    let mut hasher = crate::ContentHasher::new(ids.len());
    hasher.bytes(&bytes);
    DrawBuffer {
        hash: hasher.finish(),
        bytes: bytes.into(),
        base: 0,
    }
}

/// Inserts a [`RenderCommand::BindDrawBuffers`] before each draw whose shaders read through
/// buffers, for each stage whose buffers differ from the ones last bound, so every draw reads the
/// ranges its own user data names. A draw whose buffers cannot all be bound is counted in
/// `refused`, with the first reason, and binds none for that stage.
pub fn bind_draw_buffers(
    commands: &mut Vec<RenderCommand>,
    (sources, index_readers, cache): (
        &BTreeMap<ResourceId, Vec<BufferSource>>,
        &std::collections::BTreeSet<ResourceId>,
        &mut BufferCache,
    ),
    memory: &impl GuestMemory,
    refused: &mut (usize, Option<&'static str>),
) {
    const STAGES: [ShaderStage; 2] = [ShaderStage::Vertex, ShaderStage::Fragment];
    let index = |stage: ShaderStage| STAGES.iter().position(|s| *s == stage);
    let mut words = [[0u32; USER_DATA_WORDS]; 2];
    let mut modules: [Option<ResourceId>; 2] = [None; 2];
    let mut bound: [Option<Vec<(u64, usize, u64)>>; 2] = [None, None];
    // Ranges read this submission: guest memory does not change while it is prepared.
    let mut read: BTreeMap<(u64, u64), Result<DrawBuffer, &'static str>> = BTreeMap::new();
    let mut out = Vec::with_capacity(commands.len());
    for command in commands.drain(..) {
        match &command {
            RenderCommand::SetUserData { stage, words: set } => {
                if let Some(at) = index(*stage) {
                    words[at] = *set;
                }
            }
            RenderCommand::BindShader { stage, shader } => {
                if let Some(at) = index(*stage) {
                    modules[at] = Some(*shader);
                }
            }
            RenderCommand::Draw { .. } | RenderCommand::DrawIndexed { .. } => {
                // The lanes a primitive shader's draw runs as vertices (D758).
                let vertices = match &command {
                    RenderCommand::Draw { vertices, .. } => *vertices,
                    RenderCommand::DrawIndexed { indices, .. } => *indices,
                    _ => 0,
                };
                for (at, stage) in STAGES.into_iter().enumerate() {
                    let empty = Vec::new();
                    let module = modules[at];
                    let list = module.and_then(|m| sources.get(&m)).unwrap_or(&empty);
                    // An indexed primitive shader reads its index buffer after its traced buffers
                    // (D740).
                    let indices = match &command {
                        RenderCommand::DrawIndexed {
                            indices,
                            index_buffer,
                            ..
                        } if module.is_some_and(|m| index_readers.contains(&m)) => Some(
                            index_buffer
                                .map(|buffer| (buffer, *indices))
                                .ok_or("an indexed draw whose index size is not read (D740)"),
                        ),
                        _ => None,
                    };
                    if list.is_empty() && indices.is_none() {
                        continue;
                    }
                    let ranges = list
                        .iter()
                        .map(|source| {
                            resolve_range(source, (&words[at], vertices), memory).map(Some)
                        })
                        .chain(indices.map(|found| {
                            found.map(|(buffer, count)| match buffer {
                                IndexBuffer::Guest { address, bytes } => {
                                    Some((address, u64::from(count) * u64::from(bytes)))
                                }
                                IndexBuffer::Counting { .. } => None,
                            })
                        }));
                    let resolved: Result<Vec<DrawBuffer>, &'static str> = ranges
                        .map(|range| {
                            let Some(range) = range? else {
                                return Ok(counting_indices(indices));
                            };
                            read.entry(range)
                                .or_insert_with(|| {
                                    let length = usize::try_from(range.1)
                                        .map_err(|_| "a draw buffer larger than this host holds")?;
                                    cache.read(range.0, length, memory)
                                })
                                .clone()
                        })
                        .collect();
                    match resolved {
                        Ok(buffers) => {
                            let key: Vec<(u64, usize, u64)> = buffers
                                .iter()
                                .map(|b| (b.hash, b.bytes.len(), b.base))
                                .collect();
                            if bound[at].as_ref() != Some(&key) {
                                out.push(RenderCommand::BindDrawBuffers { stage, buffers });
                                bound[at] = Some(key);
                            }
                        }
                        Err(why) => {
                            refused.0 += 1;
                            refused.1.get_or_insert(why);
                        }
                    }
                }
            }
            _ => {}
        }
        out.push(command);
    }
    *commands = out;
}

#[cfg(test)]
mod tests {
    use super::descriptor_extent;
    use orbistoun_translate::draw_buffers::DescriptorReads;

    /// radeonsi's raw out-of-bounds mode, in a descriptor's fourth word: `OOB_SELECT` 3 in bits
    /// 29:28 (Mesa `gfx10-rsrc.json`, `SQ_BUF_RSRC_WORD3`), as its fast-path constant buffer has.
    const RAW: u32 = 0x3101_6fac;
    /// The same flags with `OOB_SELECT` 0, structured with offset.
    const STRUCTURED_WITH_OFFSET: u32 = RAW & !(3 << 28);
    /// With `OOB_SELECT` 1, structured.
    const STRUCTURED: u32 = STRUCTURED_WITH_OFFSET | 1 << 28;

    fn scalar(reach: u32) -> DescriptorReads {
        DescriptorReads {
            scalar_reach: reach,
            ..DescriptorReads::default()
        }
    }

    fn vector(reach: Option<u32>, indexed: bool) -> DescriptorReads {
        DescriptorReads {
            scalar_reach: 0,
            vector: true,
            vector_reach: reach,
            indexed,
        }
    }

    /// The clear's constant buffer: sixteen bytes, all of which its one load reads.
    #[test]
    fn a_scalar_load_reaches_the_record_count_or_its_own_end() {
        assert_eq!(descriptor_extent([0, 4, 16, RAW], scalar(16)), Ok(16));
        assert_eq!(descriptor_extent([0, 4, 64, RAW], scalar(16)), Ok(16));
        assert_eq!(descriptor_extent([0, 4, 8, RAW], scalar(16)), Ok(8));
    }

    /// Through a descriptor with a stride the record count is in units of the stride (radeonsi
    /// `si_state.c:3506-3527`, GFX6-7 and 10, and GFX9's SMEM), so a scalar load reaches its own end
    /// or the records' bytes: the AGC formatted copy's constants, one sixteen-byte record read for
    /// eight bytes (PPSA03416), reach eight.
    #[test]
    fn a_scalar_load_through_a_stride_counts_records_in_strides() {
        assert_eq!(
            descriptor_extent([0, 4 | 16 << 16, 16, RAW], scalar(16)),
            Ok(16)
        );
        assert_eq!(
            descriptor_extent([0, 4 | 16 << 16, 1, RAW], scalar(8)),
            Ok(8)
        );
        assert_eq!(
            descriptor_extent([0, 4 | 16 << 16, 1, RAW], scalar(32)),
            Ok(16)
        );
    }

    /// An indexed fetch reaches the last record's start plus its own reach into a record.
    #[test]
    fn an_indexed_fetch_reaches_into_the_last_record() {
        let stride = 24;
        let words = |flags| [0, 4 | stride << 16, 10, flags];
        assert_eq!(
            descriptor_extent(words(STRUCTURED), vector(Some(12), true)),
            Ok(9 * 24 + 12)
        );
        assert_eq!(
            descriptor_extent(words(STRUCTURED_WITH_OFFSET), vector(Some(12), true)),
            Ok(9 * 24 + 12)
        );
        // An offset in a register is bounded by the stride only where the mode checks it.
        assert_eq!(
            descriptor_extent(words(STRUCTURED_WITH_OFFSET), vector(None, true)),
            Ok(9 * 24 + 23 + 16)
        );
        assert!(descriptor_extent(words(STRUCTURED), vector(None, true)).is_err());
    }

    /// A raw-checked buffer indexed by record has no bound on the index, and is refused.
    #[test]
    fn a_raw_buffer_indexed_by_record_is_refused() {
        assert!(descriptor_extent([0, 4 | 24 << 16, 10, RAW], vector(Some(4), true)).is_err());
        assert_eq!(
            descriptor_extent([0, 4, 100, RAW], vector(None, false)),
            Ok(100)
        );
    }

    /// Swizzled records are refused rather than bound as a plain range.
    #[test]
    fn swizzled_addressing_is_refused() {
        assert!(descriptor_extent([0, 4 | 1 << 31, 16, RAW], scalar(16)).is_err());
    }

    /// Guest memory at one address.
    struct At(u64, Vec<u8>);
    impl crate::pipeline::GuestMemory for At {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            let start = usize::try_from(address.checked_sub(self.0)?).ok()?;
            self.1.get(start..start.checked_add(length)?)
        }
    }

    /// A computed descriptor (D753) is found by running the program before the access over the
    /// draw's user data and memory: here a load of the descriptor from the table the user data
    /// names. Its range runs from the descriptor's base through the scalar offset the access adds
    /// and the extent past it, and its fourth word is the format a format load reads.
    #[test]
    fn a_computed_descriptor_binds_from_its_base_past_the_offset() {
        use orbistoun_translate::draw_buffers::{BufferSource, Computed};
        let mut table = Vec::new();
        for word in [0x2000u32, 16 << 16, 4, STRUCTURED] {
            table.extend(word.to_le_bytes());
        }
        let memory = At(0x1000, table);
        let source = BufferSource::Computed(Box::new(Computed {
            // s_load_dwordx4 s[0:3], s[4:5], 0x0; s_waitcnt lgkmcnt(0)
            prefix: vec![0xf408_0002, 0xfa00_0000, 0xbf8c_c07f],
            at: 12,
            through: orbistoun_translate::draw_buffers::ComputedAccess::Descriptor {
                first: 0,
                reads: vector(Some(16), true),
            },
            // The inline constant 16.
            soffset: 128 + 16,
            first_register: 4,
            user_data: 2,
        }));
        let mut user_data = [0u32; super::USER_DATA_WORDS];
        user_data[0] = 0x1000;
        assert_eq!(
            super::resolve_range(&source, (&user_data, 0), &memory),
            Ok((0x2000, 16 + 4 * 16)),
            "four sixteen-byte records past a sixteen-byte offset"
        );
        assert_eq!(
            super::resolve_fourth_word(&source, &user_data, &memory),
            Some(STRUCTURED)
        );
        // A scalar load's base pair, computed the same way, binds from the base past the offset,
        // as far as the load reaches.
        let pointer = BufferSource::Computed(Box::new(Computed {
            prefix: Vec::new(),
            at: 0,
            through: orbistoun_translate::draw_buffers::ComputedAccess::Pointer {
                base: 4,
                end: 16,
            },
            soffset: 128 + 16,
            first_register: 4,
            user_data: 2,
        }));
        assert_eq!(
            super::resolve_range(&pointer, (&user_data, 0), &memory),
            Ok((0x1010, 16))
        );
        assert_eq!(
            super::resolve_fourth_word(&pointer, &user_data, &memory),
            None
        );
        // A table the user data does not name leaves the descriptor unknown.
        user_data[0] = 0x9000;
        assert!(super::resolve_range(&source, (&user_data, 0), &memory).is_err());
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

    /// A primitive shader's global loads (D758) bind, for each draw, the range its vertex lanes'
    /// addresses span: here three vertices from the draw's base vertex, `stride` apart, each read
    /// sixteen bytes wide. A draw of more vertices than the wave holds is refused.
    #[test]
    fn a_global_load_binds_the_range_its_vertex_lanes_read() {
        let encodings = orbistoun_shader::EncodingTable::builtin().expect("encodings");
        let operands = orbistoun_shader::OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = GL_FETCH.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = orbistoun_shader::decode_program(&bytes, &encodings, &operands);
        let traced =
            orbistoun_translate::draw_buffers::trace(&decode, &encodings, (8, 5), (false, true));
        // The attribute table, then the global loads.
        let [
            _,
            orbistoun_translate::draw_buffers::BufferSource::Global(global),
        ] = traced.sources.as_slice()
        else {
            panic!("the table and the global loads: {:?}", traced.sources);
        };
        let mut global = global.clone();
        global.width = 32;
        let source = orbistoun_translate::draw_buffers::BufferSource::Global(global);

        let (table, vertices, stride) = (0x2000_0000u64, 0x74_3000_0000u64, 28u64);
        let mut words = Vec::new();
        for word in [vertices as u32, (vertices >> 32) as u32, stride as u32, 4] {
            words.extend(word.to_le_bytes());
        }
        let memory = At(table, words);
        let mut user_data = [0u32; super::USER_DATA_WORDS];
        user_data[2] = table as u32;
        user_data[4] = 3;
        assert_eq!(
            super::resolve_range(&source, (&user_data, 3), &memory),
            Ok((vertices + 3 * stride, 2 * stride + 16))
        );
        assert!(super::resolve_range(&source, (&user_data, 33), &memory).is_err());
    }

    /// An indexed draw of a primitive shader that reads its vertex ids from its index buffer binds
    /// that buffer's indices after the module's traced buffers - here it has none - and a module
    /// that does not read them binds nothing (D740).
    #[test]
    fn an_index_reading_draw_binds_its_index_buffer() {
        use crate::backend::{IndexBuffer, RenderCommand, ResourceId, ShaderStage};
        let (reader, other) = (ResourceId(3), ResourceId(4));
        let indices: Vec<u8> = [5u16, 9, 7].iter().flat_map(|i| i.to_le_bytes()).collect();
        let memory = At(0x2000, indices.clone());
        let bind = |module| {
            let mut commands = vec![
                RenderCommand::BindShader {
                    stage: ShaderStage::Vertex,
                    shader: module,
                },
                RenderCommand::DrawIndexed {
                    indices: 3,
                    instances: 1,
                    first_index: 0,
                    index_buffer: Some(IndexBuffer::Guest {
                        address: 0x2000,
                        bytes: 2,
                    }),
                },
            ];
            super::bind_draw_buffers(
                &mut commands,
                (
                    &std::collections::BTreeMap::new(),
                    &std::collections::BTreeSet::from([reader]),
                    &mut super::BufferCache::default(),
                ),
                &memory,
                &mut (0, None),
            );
            commands
                .into_iter()
                .filter_map(|command| match command {
                    RenderCommand::BindDrawBuffers { stage, buffers } => Some((stage, buffers)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let bound = bind(reader);
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].0, ShaderStage::Vertex);
        // Six bytes, padded to whole words.
        assert_eq!(bound[0].1[0].bytes[..6], indices[..]);
        assert!(
            bind(other).is_empty(),
            "a module that does not read indices binds none"
        );
    }

    /// A chunk of a non-indexed draw binds its own run of vertex ids, counting up from its first,
    /// as thirty-two-bit indices (D741).
    #[test]
    fn a_counting_chunk_binds_its_run_of_vertex_ids() {
        use crate::backend::{IndexBuffer, RenderCommand, ResourceId, ShaderStage};
        let reader = ResourceId(3);
        let mut commands = vec![
            RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: reader,
            },
            RenderCommand::DrawIndexed {
                indices: 3,
                instances: 1,
                first_index: 0,
                index_buffer: Some(IndexBuffer::Counting { first: 63 }),
            },
        ];
        super::bind_draw_buffers(
            &mut commands,
            (
                &std::collections::BTreeMap::new(),
                &std::collections::BTreeSet::from([reader]),
                &mut super::BufferCache::default(),
            ),
            &At(0, Vec::new()),
            &mut (0, None),
        );
        let RenderCommand::BindDrawBuffers { buffers, .. } = &commands[1] else {
            panic!("no buffers bound: {commands:?}");
        };
        let expected: Vec<u8> = [63u32, 64, 65]
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();
        assert_eq!(buffers[0].bytes[..], expected[..]);
    }

    /// Each clear draw binds the sixteen bytes its user-data word 2 names, once while they are the
    /// same bytes and again when a draw names others; a draw naming unmapped memory binds nothing
    /// and is counted.
    #[test]
    fn each_draw_binds_the_range_its_user_data_names() {
        use crate::backend::{RenderCommand, ResourceId, ShaderStage, USER_DATA_WORDS};
        use orbistoun_translate::draw_buffers::{BufferSource, DescriptorWord};

        const HIGH: u64 = 4 << 32;
        let module = ResourceId(7);
        let sources = std::collections::BTreeMap::from([(
            module,
            vec![BufferSource::Descriptor {
                words: [
                    DescriptorWord::UserData(2),
                    DescriptorWord::Constant(4),
                    DescriptorWord::Constant(16),
                    DescriptorWord::Constant(RAW),
                ],
                reads: scalar(16),
            }],
        )]);
        let colours: Vec<u8> = (0u8..32).collect();
        let memory = At(HIGH | 0x1000, colours.clone());
        let draw_at = |low: u32| {
            let mut words = [0u32; USER_DATA_WORDS];
            words[2] = low;
            [
                RenderCommand::SetUserData {
                    stage: ShaderStage::Fragment,
                    words,
                },
                RenderCommand::Draw {
                    vertices: 3,
                    instances: 1,
                    first_vertex: 0,
                },
            ]
        };
        let mut commands = vec![RenderCommand::BindShader {
            stage: ShaderStage::Fragment,
            shader: module,
        }];
        for low in [0x1000, 0x1000, 0x1010, 0x9000] {
            commands.extend(draw_at(low));
        }
        let mut refused = (0, None);
        super::bind_draw_buffers(
            &mut commands,
            (
                &sources,
                &std::collections::BTreeSet::new(),
                &mut super::BufferCache::default(),
            ),
            &memory,
            &mut refused,
        );
        let bound: Vec<&[u8]> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::BindDrawBuffers { stage, buffers } => {
                    assert_eq!(*stage, ShaderStage::Fragment);
                    Some(&*buffers[0].bytes)
                }
                _ => None,
            })
            .collect();
        assert_eq!(bound, [&colours[..16], &colours[16..]]);
        assert_eq!(refused.0, 1, "the draw naming unmapped memory");
        assert!(refused.1.is_some());
    }
}
