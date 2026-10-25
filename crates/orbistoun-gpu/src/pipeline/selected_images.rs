//! The images a dispatch selects from a descriptor table at run time, every one bound (D983).
//!
//! A program loads an image descriptor's eight words through a table's descriptor at an offset it
//! computes from a lane's value, and samples through them. No host run of the program can know
//! which record a wave selects, so the host reads every record the table holds, binds the image
//! each one names in arrays the module indexes as it runs, and hands the module a directory of
//! their descriptors (`orbistoun_translate::wavefront::selected_directory`). A record whose image
//! cannot be bound is left out of the directory: a wave selecting it escapes, and the dispatch's
//! result is refused (D950).

use std::collections::BTreeMap;

use orbistoun_translate::draw_buffers::{DescriptorWord, SelectedImage, SelectedTable};
use orbistoun_translate::wavefront::{TableBase, TableWord, TextureSource};

use super::{
    DetiledImages, DispatchTexels, GuestMemory, IMAGE_TYPE_2D, IMAGE_TYPE_CUBE, TexelFormat,
    fetched_image,
};
use crate::backend::USER_DATA_WORDS;

/// Bytes between a table's records: an image descriptor's eight words, the stride the program's
/// offsets step by.
const RECORD_BYTES: u64 = 32;

/// The most records of one table read. A larger table is refused with what the records read hold.
const MOST_TABLE_RECORDS: u64 = 1 << 16;

/// The most bytes of texels a dispatch's selected images take together; past this the dispatch is
/// refused with what its tables hold.
const MOST_SELECTED_BYTES: u64 = 1 << 30;

/// A dispatch's selected images: the texture slots that read them, each image in its place in the
/// arrays, and the directory the module finds them in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectedImages {
    /// The texture slots, one bit each, whose own binding holds their sampler alone: the image
    /// comes from the arrays.
    pub slots: u32,
    /// Each image, at its place in the arrays: a flat image or a cube (its texels' `cube`), with
    /// where it was read.
    pub images: Vec<(DispatchTexels, (u64, u64))>,
    /// The directory of their descriptors, each found at its place.
    pub directory: Vec<u32>,
    /// Each image's place among the images the device keeps, where it has one, in the order of
    /// `images` (D985): one taken from the device has no texels there.
    pub resident: Vec<Option<crate::resident::Use>>,
}

/// What the tables a dispatch selects images from hold, for its note and any refusal.
#[derive(Debug, Clone, Default)]
struct Census {
    records: u64,
    named: u64,
    distinct: u64,
    bound: u64,
    bytes: u64,
    left_out: BTreeMap<String, u64>,
    formats: BTreeMap<u32, u64>,
}

impl Census {
    fn describe(&self) -> String {
        let left: Vec<String> = self
            .left_out
            .iter()
            .map(|(why, count)| format!("{count} {why}"))
            .collect();
        let formats: Vec<String> = self
            .formats
            .iter()
            .map(|(format, count)| format!("{format}x{count}"))
            .collect();
        format!(
            "of {} records, {} name an image, {} distinct, {} bound ({} MiB of texels) in formats {}; left out: {}",
            self.records,
            self.named,
            self.distinct,
            self.bound,
            self.bytes >> 20,
            formats.join(" "),
            if left.is_empty() {
                "none".to_owned()
            } else {
                left.join(", ")
            }
        )
    }
}

/// The selected images of a dispatch whose texture slots are `textures`, read where its user data
/// names their tables: `None` where no slot reads one.
///
/// # Errors
///
/// A table whose descriptor cannot be read or that holds more than [`MOST_TABLE_RECORDS`] records,
/// more distinct images than the arrays hold, or more bytes than [`MOST_SELECTED_BYTES`]; each
/// says what the tables hold.
pub(super) fn selected_images(
    state: &crate::dispatch::DispatchState,
    textures: &[TextureSource],
    memory: &impl GuestMemory,
    detiled: &mut DetiledImages,
) -> Result<Option<SelectedImages>, String> {
    let selecting: Vec<(&TextureSource, SelectedImage)> = textures
        .iter()
        .filter_map(|source| Some((source, source.computed?.selected?)))
        .collect();
    if selecting.is_empty() {
        return Ok(None);
    }
    let slots = selecting
        .iter()
        .fold(0_u32, |slots, (source, _)| slots | 1 << source.slot);
    let (flat, cube) = (
        selecting.iter().any(|(source, _)| !source.cube),
        selecting.iter().any(|(source, _)| source.cube),
    );
    let mut tables: Vec<SelectedTable> = selecting.iter().map(|(_, s)| s.table).collect();
    tables.sort_unstable();
    tables.dedup();
    let user_data = user_words(state);
    let mut census = Census::default();
    let mut seen: std::collections::BTreeSet<[u32; 8]> = std::collections::BTreeSet::new();
    let mut found = SelectedImages {
        slots,
        ..SelectedImages::default()
    };
    // Every distinct record of a shape some slot reads, first, so a dispatch whose images would
    // take too many bytes is refused before any is read.
    let mut candidates: Vec<(u64, [u32; 8], bool)> = Vec::new();
    let mut estimate = 0_u64;
    for table in tables {
        let (base, bytes) = table_extent(table, &user_data, memory)?;
        let records = bytes.saturating_sub(u64::from(table.element)) / RECORD_BYTES;
        if records > MOST_TABLE_RECORDS {
            return Err(format!(
                "a table of {records} image descriptors, more than the {MOST_TABLE_RECORDS} read (D983)"
            ));
        }
        census.records += records;
        for record in 0..records {
            let at = base + u64::from(table.element) + record * RECORD_BYTES;
            let Some(words) = record_words(at, memory) else {
                *census.left_out.entry("unmapped".to_owned()).or_default() += 1;
                continue;
            };
            if is_null(&words) {
                continue;
            }
            census.named += 1;
            if !seen.insert(words) {
                continue;
            }
            census.distinct += 1;
            let kind = words[3] >> 28;
            let is_cube = kind == IMAGE_TYPE_CUBE;
            if !((kind == IMAGE_TYPE_2D && flat) || (is_cube && cube)) {
                *census
                    .left_out
                    .entry(format!("of type {kind}"))
                    .or_default() += 1;
                continue;
            }
            *census.formats.entry((words[1] >> 20) & 0x1ff).or_default() += 1;
            // At least a word a texel of the base level, as a dispatch image's texels are.
            let descriptor = super::decode_image_descriptor(words);
            let faces = if is_cube { 6 } else { 1 };
            estimate += u64::from(descriptor.width) * u64::from(descriptor.height) * faces * 4;
            candidates.push((at, words, is_cube));
        }
    }
    census.bytes = estimate;
    if estimate > MOST_SELECTED_BYTES {
        return Err(format!(
            "the images selected at run time take more than {} MiB (D983): {}",
            MOST_SELECTED_BYTES >> 20,
            census.describe()
        ));
    }
    census.bytes = 0;
    let mut cubes = Vec::new();
    for (at, words, is_cube) in candidates {
        let source = record_source(at, is_cube);
        let mut used = None;
        match fetched_image(state, &source, memory, detiled, Some(&mut used)) {
            Ok((texels, span)) if !texels.format.is_integer() => {
                census.bound += 1;
                census.bytes += texels.texels.len() as u64 * 4;
                found.images.push((texels, span));
                found.resident.push(used);
                cubes.push((words, is_cube));
            }
            Ok(_) => *census.left_out.entry("integer".to_owned()).or_default() += 1,
            Err(why) => *census.left_out.entry(reason(&why)).or_default() += 1,
        }
    }
    found.directory =
        orbistoun_translate::wavefront::selected_directory(&cubes).ok_or_else(|| {
            format!(
                "more images selected at run time than the {} the arrays hold (D983): {}",
                orbistoun_translate::wavefront::MOST_SELECTED_IMAGES,
                census.describe()
            )
        })?;
    note_census(state.program, &census);
    Ok(Some(found))
}

/// The stage's user-data words, a word the stream never wrote as zero, as a buffer's are.
fn user_words(state: &crate::dispatch::DispatchState) -> [u32; USER_DATA_WORDS] {
    let mut user = [0_u32; USER_DATA_WORDS];
    for (word, value) in user.iter_mut().zip(&state.user_data) {
        *word = value.unwrap_or(0);
    }
    user
}

/// Where `table`'s records start and how many bytes its descriptor admits.
fn table_extent(
    table: SelectedTable,
    user: &[u32; USER_DATA_WORDS],
    memory: &impl GuestMemory,
) -> Result<(u64, u64), String> {
    let mut words = [0_u32; 4];
    for (value, word) in words.iter_mut().zip(table.words) {
        *value = crate::draw_buffers::resolve_word(word, user, memory)
            .map_err(|why| format!("the image table's descriptor: {why} (D983)"))?;
    }
    let descriptor = crate::registers::decode_buffer_descriptor(words);
    Ok((
        crate::pipeline::guest_address_of(descriptor.base),
        descriptor.byte_len(),
    ))
}

/// The eight words of the record at `at`, where they are mapped.
fn record_words(at: u64, memory: &impl GuestMemory) -> Option<[u32; 8]> {
    let bytes = memory.read(at, 32)?;
    let mut words = [0_u32; 8];
    for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
        *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    Some(words)
}

/// Whether a record names no image: every word zero, or a base of zero, which a table's unused
/// records hold.
fn is_null(words: &[u32; 8]) -> bool {
    words.iter().all(|&word| word == 0) || (words[0] == 0 && words[1].trailing_zeros() >= 8)
}

/// The texture source a record at `at` is read through: its eight words at a constant address, in
/// the shape its type names.
fn record_source(at: u64, cube: bool) -> TextureSource {
    TextureSource {
        table: TableBase {
            low: TableWord::Constant(at as u32),
            high: TableWord::Constant((at >> 32) as u32),
        },
        table_offset: Some(0),
        cube,
        ..TextureSource::default()
    }
}

/// A refusal's reason without the numbers that make every record's differ, so the census counts
/// records by kind.
fn reason(why: &str) -> String {
    let words: Vec<&str> = why
        .split_whitespace()
        .filter(|word| !word.chars().any(|c| c.is_ascii_digit()))
        .take(12)
        .collect();
    words.join(" ")
}

/// The sampler a slot reading selected images samples through, read from where the program formed
/// it (D983): `None` for a slot that only fetches.
///
/// # Errors
///
/// A slot that samples through a sampler the program computes from what the host cannot know, or
/// one that cannot be read.
pub(super) fn selected_sampling(
    state: &crate::dispatch::DispatchState,
    selected: SelectedImage,
    memory: &impl GuestMemory,
) -> Result<Option<crate::registers::TextureSampling>, String> {
    let Some(sampler) = selected.sampler else {
        return if selected.sampled {
            Err(concat!(
                "an image selected at run time is sampled through a sampler the program ",
                "computes from what the host cannot know (D983)"
            )
            .to_owned())
        } else {
            Ok(None)
        };
    };
    let user = user_words(state);
    let mut words = [0_u32; 4];
    for (value, word) in words.iter_mut().zip(sampler) {
        *value = resolve(word, &user, memory)?;
    }
    crate::registers::decode_sampler_descriptor(words)
        .map(Some)
        .map_err(|why| format!("its sampler: {why}"))
}

/// One traced descriptor word's value at this dispatch.
fn resolve(
    word: DescriptorWord,
    user: &[u32; USER_DATA_WORDS],
    memory: &impl GuestMemory,
) -> Result<u32, String> {
    crate::draw_buffers::resolve_word(word, user, memory)
        .map_err(|why| format!("its sampler: {why}"))
}

/// The placeholder a slot reading selected images binds at its own binding, where only its sampler
/// is read: one texel, with the slot's sampler.
pub(super) fn sampler_slot(sampling: Option<crate::registers::TextureSampling>) -> DispatchTexels {
    DispatchTexels {
        texels: vec![0].into(),
        format: TexelFormat::Rgba8,
        width: 1,
        height: 1,
        depth: None,
        layers: None,
        sampling,
        cube: false,
    }
}

/// Says once per program and image count what its tables held.
fn note_census(program: u64, census: &Census) {
    static NOTED: std::sync::Mutex<Vec<(u64, u64)>> = std::sync::Mutex::new(Vec::new());
    let key = (program, census.bound);
    let fresh = NOTED.lock().is_ok_and(|mut noted| {
        !noted.contains(&key) && {
            noted.push(key);
            true
        }
    });
    if fresh {
        tracing::info!(
            "the dispatch program at {program:#x} binds the images it selects at run time (D983): {}",
            census.describe()
        );
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_translate::Width;
    use orbistoun_translate::draw_buffers::{DescriptorWord, SelectedImage, SelectedTable};
    use orbistoun_translate::wavefront::{
        ComputeInputs, ComputedImage, SELECTED_CUBE_TAG, SELECTED_DIRECTORY_PROBES,
        SELECTED_DIRECTORY_SLOT_WORDS, TextureSource, selected_image_hash,
    };

    use super::super::{DetiledImages, GuestMemory};
    use crate::dispatch::DispatchState;

    const TABLE: u64 = 0x1_0000;
    const IMAGES: u64 = 0x2_0000;
    /// Texels across each image: one 256-byte row of words, whatever a linear pitch rounds to.
    const WIDE: u32 = 64;

    struct Memory(Vec<(u64, Vec<u8>)>);

    impl GuestMemory for Memory {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            self.0.iter().find_map(|(base, bytes)| {
                let start = usize::try_from(address.checked_sub(*base)?).ok()?;
                bytes.get(start..start.checked_add(length)?)
            })
        }
    }

    /// Image `n`'s descriptor: a linear `32_FLOAT` row of [`WIDE`] texels, read through `X`.
    fn image(n: u64) -> [u32; 8] {
        let base = IMAGES + n * 0x1000;
        [
            (base >> 8) as u32,
            super::super::FORMAT_32_FLOAT << 20 | ((WIDE - 1) & 3) << 30,
            (WIDE - 1) >> 2,
            4 | 5 << 3 | 6 << 6 | 7 << 9 | super::IMAGE_TYPE_2D << 28,
            0,
            0,
            0,
            0,
        ]
    }

    /// The texel image `n` holds at `x`, as a float's bits.
    fn texel(n: u64, x: u32) -> u32 {
        let (n, x) = (
            u16::try_from(n).expect("small"),
            u16::try_from(x).expect("small"),
        );
        (f32::from(n) * 1000.0 + f32::from(x)).to_bits()
    }

    /// Six records: images 0 to 3, a null record, and image 1 again.
    fn memory() -> Memory {
        let records = [image(0), image(1), image(2), image(3), [0; 8], image(1)];
        let table: Vec<u8> = records
            .iter()
            .flatten()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let images: Vec<u8> = (0..4)
            .flat_map(|n| {
                let mut page = vec![0_u8; 0x1000];
                for x in 0..WIDE {
                    let at = x as usize * 4;
                    page[at..at + 4].copy_from_slice(&texel(n, x).to_le_bytes());
                }
                page
            })
            .collect();
        Memory(vec![(TABLE, table), (IMAGES, images)])
    }

    fn state() -> DispatchState {
        // A raw buffer descriptor of the table's six records in user data 0 to 3.
        let user_data = [TABLE as u32, 0, 6 * 32, 0x3101_6fac]
            .into_iter()
            .map(Some)
            .chain(std::iter::repeat_n(Some(0), 12))
            .collect();
        DispatchState {
            program: 0x9000,
            user_data,
            inputs: ComputeInputs::default(),
            width: Width::Wave64,
            dx10_clamp: None,
            local_words: None,
            groups: [1, 1, 1],
            vertex_streamout: false,
        }
    }

    fn selecting(sampler: Option<[DescriptorWord; 4]>) -> TextureSource {
        TextureSource {
            slot: 1,
            computed: Some(ComputedImage {
                at: 0x40,
                register: 24,
                first_register: 0,
                selected: Some(SelectedImage {
                    table: SelectedTable {
                        words: [0, 1, 2, 3].map(DescriptorWord::UserData),
                        element: 0,
                    },
                    sampler,
                    sampled: true,
                }),
            }),
            ..TextureSource::default()
        }
    }

    /// Every distinct image the table's records name is bound once, a null record none, and the
    /// directory finds each record's descriptor at the place of the image bound for it.
    #[test]
    fn every_image_a_table_names_is_bound_and_found_by_its_descriptor() {
        let sampler = Some([DescriptorWord::Constant(0); 4]);
        let sources = [TextureSource::default(), selecting(sampler)];
        let found =
            super::selected_images(&state(), &sources, &memory(), &mut DetiledImages::default())
                .expect("bound")
                .expect("a slot selects");
        assert_eq!(found.slots, 1 << 1, "slot 1 binds its sampler alone");
        assert_eq!(found.images.len(), 4, "four distinct images");
        let mask = found.directory[0];
        let width = SELECTED_DIRECTORY_SLOT_WORDS as usize;
        for n in 0..4 {
            let words = image(n);
            let home = selected_image_hash(&words) & mask;
            let tag = (0..SELECTED_DIRECTORY_PROBES)
                .find_map(|probe| {
                    let at = 1 + ((home + probe) & mask) as usize * width;
                    (found.directory[at..at + 8] == words[..]).then(|| found.directory[at + 8])
                })
                .expect("in the directory");
            assert_eq!(tag & SELECTED_CUBE_TAG, 0, "a flat image");
            let (texels, _) = &found.images[(tag - 1) as usize];
            let expected: Vec<u32> = (0..WIDE).map(|x| texel(n, x)).collect();
            assert_eq!(&texels.texels[..WIDE as usize], &expected[..], "image {n}");
        }
        let sampling = super::selected_sampling(
            &state(),
            sources[1]
                .computed
                .and_then(|c| c.selected)
                .expect("selected"),
            &memory(),
        )
        .expect("read");
        assert!(sampling.is_some(), "the constant sampler is read");
    }

    /// A compared slot reading selected images compares in the module by the function its traced
    /// sampler names (D976): Grand Theft Auto V's shadow compares at `0x74005b858800` build theirs
    /// from constants, function 3 in its first word.
    #[test]
    fn a_selected_compare_compares_by_its_sampler_s_function() {
        let sampler = [0x31b6, 0x00ff_f000, 0x0650_0000, 0x8000_0000].map(DescriptorWord::Constant);
        let sources = [
            TextureSource::default(),
            TextureSource {
                compare: true,
                ..selecting(Some(sampler))
            },
        ];
        let compared =
            super::super::dispatch_compared(&state(), &sources, &memory()).expect("read");
        // `XY_MAG_FILTER` 1 in the third word, bits 21:20: bilinear.
        assert_eq!(
            compared[1],
            orbistoun_translate::wavefront::SHADER_COMPARE
                | orbistoun_translate::wavefront::SHADER_COMPARE_LINEAR
                | 3,
            "function 3, filtered"
        );
    }

    /// A slot sampling through a sampler the program computes is refused by name: the host cannot
    /// know which sampler to bind.
    #[test]
    fn a_sampler_the_host_cannot_know_is_refused() {
        let selected = selecting(None)
            .computed
            .and_then(|c| c.selected)
            .expect("selected");
        let refused = super::selected_sampling(&state(), selected, &memory()).expect_err("refused");
        assert!(refused.contains("cannot know"), "{refused}");
    }
}
