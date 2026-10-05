//! A draw's buffers resolved from guest memory (D733).
//!
//! The translator names each buffer a module reads through by where its descriptor or base comes
//! from ([`BufferSource`]); per draw, this finds the guest range each names - from the draw's user
//! data, constants and the descriptor-table words it reads - and the bytes the descriptor's bounds
//! rules let the module reach, and hands them to the backend as a
//! [`RenderCommand::BindDrawBuffers`]. A range that cannot be bound exactly is refused by name.

use std::collections::BTreeMap;

use orbistoun_translate::draw_buffers::{BufferSource, DescriptorReads, DescriptorWord};
use orbistoun_translate::wavefront::{TableBase, TableWord};

use crate::backend::{DrawBuffer, RenderCommand, ResourceId, ShaderStage, USER_DATA_WORDS};
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
    // A scalar buffer load reads each word within the record count in bytes, and zero through a
    // descriptor with a stride (the translator's `scalar_buffer_load`).
    let scalar = if stride == 0 {
        records.min(u64::from(reads.scalar_reach))
    } else {
        0
    };
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

/// The guest range a source names for a draw with these user-data words: its address and length.
///
/// # Errors
///
/// A refusal naming why the range cannot be bound exactly.
fn resolve_range(
    source: &BufferSource,
    user_data: &[u32; USER_DATA_WORDS],
    memory: &impl GuestMemory,
) -> Result<(u64, u64), &'static str> {
    let (address, length) = match *source {
        BufferSource::Pointer { base, extent } => {
            (table_address(base, user_data), u64::from(extent))
        }
        BufferSource::Descriptor { words, reads } => {
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
    let mut bound: [Option<Vec<(u64, usize)>>; 2] = [None, None];
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
                                .map(|(address, bytes)| {
                                    (address, u64::from(*indices) * u64::from(bytes))
                                })
                                .ok_or("an indexed draw whose index size is not read (D740)"),
                        ),
                        _ => None,
                    };
                    if list.is_empty() && indices.is_none() {
                        continue;
                    }
                    let ranges = list
                        .iter()
                        .map(|source| resolve_range(source, &words[at], memory))
                        .chain(indices);
                    let resolved: Result<Vec<DrawBuffer>, &'static str> = ranges
                        .map(|range| {
                            let range = range?;
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
                            let key: Vec<(u64, usize)> =
                                buffers.iter().map(|b| (b.hash, b.bytes.len())).collect();
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

    /// A scalar load through a descriptor with a stride reads zero, so it reaches nothing.
    #[test]
    fn a_scalar_load_through_a_stride_reaches_nothing() {
        assert_eq!(
            descriptor_extent([0, 4 | 16 << 16, 16, RAW], scalar(16)),
            Ok(0)
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

    /// An indexed draw of a primitive shader that reads its vertex ids from its index buffer binds
    /// that buffer's indices after the module's traced buffers - here it has none - and a module
    /// that does not read them binds nothing (D740).
    #[test]
    fn an_index_reading_draw_binds_its_index_buffer() {
        use crate::backend::{RenderCommand, ResourceId, ShaderStage};
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
                    index_buffer: Some((0x2000, 2)),
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
