//! A descriptor table a dispatch selects a record of at run time, bound whole (D966).
//!
//! A program loads four words through a table's descriptor at an offset it computes - from a lane's
//! value, which no host run of the program can know - and reads a buffer through them. The table's
//! own descriptor is traced, so the host reads every record it holds: each record's descriptor
//! names the range an access through it can reach, and those ranges' pages go into the dispatch's
//! global buffer of pages (D961). The module reads through whichever record the program selected,
//! at the guest address that record names, under that record's own bounds.

use orbistoun_translate::draw_buffers::{DescriptorReads, SelectedTable};

use super::{MOST_BYTES, Refusal, descriptor_extent, resolve_word};
use crate::backend::USER_DATA_WORDS;
use crate::pipeline::GuestMemory;
use crate::registers::decode_buffer_descriptor;

/// The most records a table bound whole holds. Each record's range is read on the host and its pages
/// bound, so the count is bounded. A larger table is not bound whole: the dispatch goes by device
/// address instead, reading each record the program selects where it lies and converting by its
/// format as the module runs (D967), so no table is refused for its size.
pub(crate) const MOST_TABLE_RECORDS: u32 = 256;

/// The bits of a descriptor's fourth word a format load converts by (D738): the channel selects,
/// bits 11:0, and the format, bits 18:12.
const FORMAT_BITS: u32 = 0x7_ffff;

/// What a table's records reach, as one access's reads through any of them reach.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TableRecords {
    /// How many records the table holds.
    pub records: u32,
    /// Each record's range that binds a byte, as `(address, bytes)`.
    pub spans: Vec<(u64, u64)>,
    /// The fourth word every record that names a buffer shares in its format and selects: what a
    /// format load through any of them converts by. Zero where no record names a buffer, which reads
    /// zero; `None` where two records disagree.
    pub format: Option<u32>,
}

/// Every record of `table` and what `reads` reach through each (D966), at a dispatch with these
/// user-data words: the table's descriptor is read from them and guest memory, and each record's
/// descriptor from the record. A record naming no buffer - a base of zero, or no bytes - binds
/// nothing, as the hardware reads zero through it.
///
/// # Errors
///
/// A table whose descriptor cannot be read, whose records are not strided widely enough to hold a
/// descriptor at the table's element, or that holds more than [`MOST_TABLE_RECORDS`]; or a record
/// that is not mapped, that asks for addressing the module does not do, or whose range is unbounded
/// or larger than one buffer binds.
pub(crate) fn table_records(
    table: SelectedTable,
    reads: DescriptorReads,
    user_data: &[u32; USER_DATA_WORDS],
    memory: &impl GuestMemory,
) -> Result<TableRecords, Refusal> {
    let mut words = [0_u32; 4];
    for (value, word) in words.iter_mut().zip(table.words) {
        *value = resolve_word(word, user_data, memory)
            .map_err(|why| -> Refusal { format!("the table's descriptor: {why}").into() })?;
    }
    let descriptor = decode_buffer_descriptor(words);
    if descriptor.unsupported {
        return Err(
            "a table whose descriptor asks for swizzled or thread-indexed addressing".into(),
        );
    }
    let stride = descriptor.stride;
    if stride < 16 || u64::from(table.element) + 16 > u64::from(stride) {
        return Err(format!(
            "a table of {stride}-byte records, which cannot hold a descriptor {:#x} bytes in",
            table.element
        )
        .into());
    }
    let base = crate::pipeline::guest_address_of(descriptor.base);
    if descriptor.records > MOST_TABLE_RECORDS {
        return Err(format!(
            "a table of {} records, more than the {MOST_TABLE_RECORDS} bound whole: {}",
            descriptor.records,
            census(
                (base, stride, table.element),
                descriptor.records,
                reads,
                memory
            )
        )
        .into());
    }
    let mut found = TableRecords {
        records: descriptor.records,
        spans: Vec::new(),
        format: Some(0),
    };
    let mut format = None;
    for record in 0..descriptor.records {
        let at = base
            .wrapping_add(u64::from(record) * u64::from(stride))
            .wrapping_add(u64::from(table.element));
        let bytes = memory.read(at, 16).ok_or_else(|| -> Refusal {
            format!("record {record}, at {at:#x}, is not mapped").into()
        })?;
        let mut held = [0_u32; 4];
        for (word, chunk) in held.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        let named = decode_buffer_descriptor(held);
        if named.base == 0 || named.byte_len() == 0 {
            continue;
        }
        let extent = descriptor_extent(held, reads).map_err(|why| -> Refusal {
            format!("record {record}, words {held:#010x?}: {why}").into()
        })?;
        if extent > MOST_BYTES {
            return Err(format!(
                "record {record} names {extent:#x} bytes, more than one buffer binds (D733)"
            )
            .into());
        }
        if extent > 0 {
            found
                .spans
                .push((crate::pipeline::guest_address_of(named.base), extent));
        }
        let word = held[3] & FORMAT_BITS;
        match format {
            None => format = Some(word),
            Some(seen) if seen != word => found.format = None,
            Some(_) => {}
        }
    }
    if found.format.is_some() {
        found.format = Some(format.unwrap_or(0));
    }
    Ok(found)
}

/// The most records of a table too large to bind whole that [`census`] reads.
const MOST_CENSUS_RECORDS: u32 = 1 << 18;

/// The most distinct pages [`census`] counts before it stops.
const MOST_CENSUS_PAGES: usize = 1 << 20;

/// What a table too large to bind whole holds, for its refusal (D966): how many of its records -
/// `records` at `base`, `stride` bytes apart, each descriptor `element` bytes in - name a buffer,
/// the distinct pages and bytes the ones `reads` bound would reach, and how many formats they
/// convert by. Read until a record is not mapped, or [`MOST_CENSUS_RECORDS`] have been.
fn census(
    (base, stride, element): (u64, u32, u32),
    records: u32,
    reads: DescriptorReads,
    memory: &impl GuestMemory,
) -> String {
    let page = crate::global_pages::PAGE;
    let (mut read, mut named, mut unbounded) = (0_u32, 0_u32, 0_u32);
    let mut pages = std::collections::BTreeSet::new();
    let mut formats = std::collections::BTreeSet::new();
    for record in 0..records.min(MOST_CENSUS_RECORDS) {
        let at = base
            .wrapping_add(u64::from(record) * u64::from(stride))
            .wrapping_add(u64::from(element));
        let Some(bytes) = memory.read(at, 16) else {
            break;
        };
        read += 1;
        let mut held = [0_u32; 4];
        for (word, chunk) in held.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        let descriptor = decode_buffer_descriptor(held);
        if descriptor.base == 0 || descriptor.byte_len() == 0 {
            continue;
        }
        named += 1;
        formats.insert(held[3] & FORMAT_BITS);
        match descriptor_extent(held, reads) {
            Ok(extent) if extent <= MOST_BYTES && pages.len() < MOST_CENSUS_PAGES => {
                let first = descriptor.base & !(page - 1);
                let last = descriptor.base.saturating_add(extent.max(1) - 1) & !(page - 1);
                let mut at = first;
                while at <= last && pages.len() < MOST_CENSUS_PAGES {
                    pages.insert(at);
                    at += page;
                }
            }
            Ok(_) => {}
            Err(_) => unbounded += 1,
        }
    }
    format!(
        "of {read} records read, {named} name a buffer, {unbounded} of them unbounded, the rest reaching {} pages ({} MiB) in {} formats",
        pages.len(),
        (pages.len() as u64 * page) >> 20,
        formats.len()
    )
}

#[cfg(test)]
mod tests {
    use super::{TableRecords, table_records};
    use crate::backend::USER_DATA_WORDS;
    use orbistoun_translate::draw_buffers::{DescriptorReads, DescriptorWord, SelectedTable};

    /// Guest memory of a few runs of bytes.
    struct Memory(Vec<(u64, Vec<u8>)>);

    impl crate::pipeline::GuestMemory for Memory {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            self.0.iter().find_map(|(base, bytes)| {
                let start = usize::try_from(address.checked_sub(*base)?).ok()?;
                bytes.get(start..start.checked_add(length)?)
            })
        }
    }

    const TABLE: u64 = 0x7400_0010_0000;

    /// A descriptor of `base`, `stride` and `records`, its fourth word `fourth`.
    fn descriptor(base: u64, stride: u32, records: u32, fourth: u32) -> [u32; 4] {
        [
            base as u32,
            (base >> 32) as u32 & 0xffff | stride << 16,
            records,
            fourth,
        ]
    }

    /// The table's descriptor in user data words 0 to 3, at `TABLE`, of `records` records of
    /// `stride` bytes.
    fn user_data(stride: u32, records: u32) -> [u32; USER_DATA_WORDS] {
        let mut words = [0; USER_DATA_WORDS];
        words[..4].copy_from_slice(&descriptor(TABLE, stride, records, 0));
        words
    }

    fn table(element: u32) -> SelectedTable {
        SelectedTable {
            words: [0, 1, 2, 3].map(DescriptorWord::UserData),
            element,
        }
    }

    /// The table's bytes: each record `stride` bytes, its descriptor `element` bytes in.
    fn records(stride: usize, element: usize, descriptors: &[[u32; 4]]) -> Vec<u8> {
        let mut bytes = vec![0xee_u8; stride * descriptors.len()];
        for (index, words) in descriptors.iter().enumerate() {
            let at = index * stride + element;
            for (word, value) in words.iter().enumerate() {
                bytes[at + 4 * word..at + 4 * word + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    /// An index fetch through any record: indexed, sixteen bytes at most from a record's start, as
    /// `buffer_load_format_x` with `idxen` reads.
    const FETCH: DescriptorReads = DescriptorReads {
        scalar_reach: 0,
        vector: true,
        vector_reach: Some(4),
        indexed: true,
        index: None,
    };

    /// `R32_UINT` with the identity selects, out-of-bounds mode 1 (structured: the index alone).
    const R32_UINT: u32 = 0x1000_0000 | 20 << 12 | 0xfac;

    /// Every record that names a buffer binds that buffer's records, wherever they lie, and one
    /// naming none binds nothing: a table of three meshes' index buffers gigabytes apart, and an
    /// empty slot between them.
    #[test]
    fn every_record_naming_a_buffer_binds_its_range() {
        let far = 0x7401_0000_0000_u64;
        let bytes = records(
            16,
            0,
            &[
                descriptor(0x7400_0020_0000, 4, 100, R32_UINT),
                [0; 4],
                descriptor(far, 4, 0x40, R32_UINT),
                descriptor(0x7400_0020_1000, 4, 3, R32_UINT),
            ],
        );
        let memory = Memory(vec![(TABLE, bytes)]);
        let found = table_records(table(0), FETCH, &user_data(16, 4), &memory).expect("bound");
        assert_eq!(
            found,
            TableRecords {
                records: 4,
                spans: vec![
                    (0x7400_0020_0000, 100 * 4),
                    (far, 0x40 * 4),
                    (0x7400_0020_1000, 3 * 4),
                ],
                format: Some(R32_UINT & 0x7_ffff),
            }
        );
    }

    /// A record's descriptor is read at the table's element, the selecting load's immediate, within
    /// each record's stride: Grand Theft Auto V's program at `0x74005b858800` selects one at +0x40 of
    /// 0x60-byte records.
    #[test]
    fn a_record_is_read_at_the_element() {
        let bytes = records(
            0x60,
            0x40,
            &[
                descriptor(0x7400_0030_0000, 4, 8, R32_UINT),
                descriptor(0x7400_0030_1000, 4, 8, R32_UINT),
            ],
        );
        let memory = Memory(vec![(TABLE, bytes)]);
        let found = table_records(table(0x40), FETCH, &user_data(0x60, 2), &memory).expect("bound");
        assert_eq!(
            found.spans,
            [(0x7400_0030_0000, 32), (0x7400_0030_1000, 32)]
        );
    }

    /// Records that convert by different formats have no one format a module converts by; records of
    /// one format, and of none, do.
    #[test]
    fn records_of_two_formats_have_no_format() {
        let float = 0x1000_0000 | 22 << 12 | 0xfac;
        let bytes = records(
            16,
            0,
            &[
                descriptor(0x7400_0020_0000, 4, 1, R32_UINT),
                descriptor(0x7400_0020_1000, 4, 1, float),
            ],
        );
        let memory = Memory(vec![(TABLE, bytes)]);
        let found = table_records(table(0), FETCH, &user_data(16, 2), &memory).expect("bound");
        assert_eq!(found.format, None);
        let empty = Memory(vec![(TABLE, records(16, 0, &[[0; 4]]))]);
        let found = table_records(table(0), FETCH, &user_data(16, 1), &empty).expect("bound");
        assert_eq!((found.spans.len(), found.format), (0, Some(0)));
    }

    /// A table is refused where it cannot be bound whole: more records than are read, records too
    /// narrow for a descriptor at the element, a record that is not mapped, and one whose reads
    /// nothing bounds.
    #[test]
    fn a_table_that_cannot_be_bound_whole_is_refused() {
        let one = records(16, 0, &[descriptor(0x7400_0020_0000, 4, 1, R32_UINT)]);
        let memory = Memory(vec![(TABLE, one.clone())]);
        assert!(table_records(table(0), FETCH, &user_data(16, 257), &memory).is_err());
        assert!(table_records(table(0), FETCH, &user_data(8, 1), &memory).is_err());
        assert!(table_records(table(4), FETCH, &user_data(16, 1), &memory).is_err());
        assert!(table_records(table(0), FETCH, &user_data(16, 2), &memory).is_err());
        // Raw, indexed with a stride: the index is unchecked (D733).
        let raw = records(16, 0, &[descriptor(0x7400_0020_0000, 4, 1, 0x3000_0000)]);
        let memory = Memory(vec![(TABLE, raw)]);
        assert!(table_records(table(0), FETCH, &user_data(16, 1), &memory).is_err());
    }

    /// A dispatch whose lanes each select their record of a table by a waterfall is prepared with
    /// every record bound in its global buffer of pages - though the program has no global access
    /// of its own - and its selected record's slot binding nothing; the module converts by the
    /// format the records share. A table too large to bind whole, or whose records disagree in
    /// format, is refused with why it was not bound, and the dispatch is then prepared
    /// device-addressed, each access converting by its own record's format (D967).
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one program and its memory, prepared each way a table is bound"
    )]
    fn a_waterfall_over_a_table_binds_every_record() {
        use crate::dispatch::DispatchState;
        use crate::pipeline::Pipeline;
        use orbistoun_translate::wavefront::ComputeInputs;
        use orbistoun_translate::{Fidelity, Strategy, Width};
        const PROGRAM: u64 = 0x1_0000;
        const DESTINATION: u64 = 0x7400_0050_0000;
        const RAW: u32 = 0x3101_6fac;
        // The waterfall of `orbistoun-translate`'s table tests: each lane's record is its id's low
        // two bits, fetched with `buffer_load_format_x` at its id's upper bits, and stored.
        let mut program: Vec<u8> = [
            0x3602_0083_u32,
            0x3406_0082,
            0x2c08_0082,
            0xbe94_047e,
            0x7e18_0501,
            0x7d84_020c,
            0xbe96_246a,
            0x8f0c_840c,
            0xf428_0200,
            0x1800_0000,
            0xbf8c_c07f,
            0xe000_2000,
            0x8002_0204,
            0x8afe_6a16,
            0xbf89_fff5,
            0xbefe_0414,
            0xbf8c_3f70,
            0xe070_1000,
            0x8001_0203,
            0xbf81_0000,
            0xbf9f_0000,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
        program.resize(crate::pipeline::MAX_SHADER_BYTES, 0);
        let record = |index: u64| 0x7400_0020_0000 + index * ((1 << 30) + 0x40);
        let state = |records: u32| {
            let mut user_data = vec![Some(0); 8];
            for (at, words) in [
                (0, descriptor(TABLE, 16, records, RAW)),
                (4, descriptor(DESTINATION, 0, 64 * 4, RAW)),
            ] {
                for (step, word) in words.into_iter().enumerate() {
                    user_data[at + step] = Some(word);
                }
            }
            DispatchState {
                program: PROGRAM,
                user_data,
                inputs: ComputeInputs {
                    thread_id_components: 1,
                    threads: [64, 1, 1],
                    ..ComputeInputs::default()
                },
                width: Width::Wave64,
                dx10_clamp: None,
                local_words: None,
                groups: [1, 1, 1],
                vertex_streamout: false,
            }
        };
        let descriptors: Vec<[u32; 4]> = (0..300)
            .map(|index| descriptor(record(index % 4), 4, 16, R32_UINT))
            .collect();
        let mut memory = vec![
            (PROGRAM, program),
            (TABLE, records(16, 0, &descriptors)),
            (DESTINATION, vec![0; 0x100]),
        ];
        memory.extend((0..4).map(|index| (record(index), vec![0x5a; 0x40])));
        let memory = Memory(memory);
        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        })
        .expect("a pipeline");
        let prepared = pipeline
            .prepare_dispatch(&state(4), &memory)
            .expect("the table is bound whole");
        let held = prepared.global_pages.as_ref().expect("a buffer of pages");
        let page = |address: u64| address & !(crate::global_pages::PAGE - 1);
        assert_eq!(
            held.pages,
            (0..4).map(|i| page(record(i))).collect::<Vec<_>>()
        );
        assert_eq!(held.slot, 3, "after the table, the record and the store");
        assert_eq!(
            prepared.buffers[1],
            (0, 0),
            "the record's own slot binds nothing"
        );
        assert_eq!(prepared.buffers[0], (TABLE, 64), "the table, read whole");
        let refused = pipeline
            .prepare_dispatch(&state(300), &memory)
            .map(|_| ())
            .expect_err("too many records to bind whole");
        assert!(
            refused
                .contains("bound as a table selected at run time (D966): a table of 300 records"),
            "{refused}"
        );
        // No table is refused for its size: the dispatch is prepared again device-addressed, and
        // its format load converts by the format its selected record names as the module runs
        // (D967).
        assert!(
            crate::device_addressed::host_cannot_know(&refused),
            "{refused}"
        );
        let device = |records: u32| {
            let mut device = state(records);
            device.inputs.device_addressed = true;
            device
        };
        let prepared = pipeline
            .prepare_dispatch(&device(300), &memory)
            .expect("prepared device-addressed");
        assert!(prepared.device_addressed);
        // Records of two formats are not bound whole where the program converts by format: their
        // accesses go by device address, each converting by its own record's format (D967).
        let mixed: Vec<[u32; 4]> = (0..4)
            .map(|index| {
                // `32_UINT` and `32_FLOAT`, each bounded by its records.
                let format = if index % 2 == 0 {
                    R32_UINT
                } else {
                    0x1000_0000 | 22 << 12 | 0xfac
                };
                descriptor(record(index), 4, 16, format)
            })
            .collect();
        let mut memory = memory;
        memory.0[1] = (TABLE, records(16, 0, &mixed));
        let refused = pipeline
            .prepare_dispatch(&state(4), &memory)
            .map(|_| ())
            .expect_err("records of two formats");
        assert!(refused.contains("more than one format"), "{refused}");
        assert!(
            crate::device_addressed::host_cannot_know(&refused),
            "{refused}"
        );
        pipeline
            .prepare_dispatch(&device(4), &memory)
            .expect("prepared device-addressed");
    }
}
