//! Dispatch images read through their descriptors: formats, and views of a chain's levels.

use crate::dispatch::DispatchState;
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{ComputeInputs, TextureSource};

/// Where Grand Theft Auto V's `8_8_8_8_SNORM` image lies.
const IMAGE_AT: u64 = 0x7400_8800_0000;

/// `X`, `Y`, `Z`, `W`: each channel as it is.
const IDENTITY: u32 = 4 | 5 << 3 | 6 << 6 | 7 << 9;

struct Memory(Vec<u8>);

impl super::GuestMemory for Memory {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        let start = usize::try_from(address.checked_sub(IMAGE_AT)?).ok()?;
        self.0.get(start..start.checked_add(length)?)
    }
}

/// A dispatch whose user data holds `words` from its first word.
fn state(words: [u32; 8]) -> DispatchState {
    DispatchState {
        program: 0x1_0000,
        user_data: words.map(Some).to_vec(),
        inputs: ComputeInputs {
            workgroup_ids: [true, true, false],
            thread_id_components: 2,
            threads: [16, 16, 1],
            ..ComputeInputs::default()
        },
        width: Width::Wave64,
        dx10_clamp: None,
        local_words: None,
        groups: [240, 135, 1],
        vertex_streamout: false,
    }
}

/// The descriptor of a `size` x `size` 2D image (`TYPE` 9) of format `format` at [`IMAGE_AT`] in swizzle
/// mode `mode`, viewing levels 0 to `last` of `levels`, through `selects` (`gfx10-rsrc.json`
/// `SQ_IMG_RSRC_WORD0`-`5`, as `decode_image_descriptor` reads them).
fn descriptor(
    format: u32,
    size: u32,
    mode: u32,
    (levels, last): (u32, u32),
    selects: u32,
) -> [u32; 8] {
    let base = IMAGE_AT >> 8;
    [
        base as u32,
        (base >> 32) as u32 | format << 20 | ((size - 1) & 3) << 30,
        (size - 1) >> 2 | (size - 1) << 14,
        selects | last << 16 | mode << 20 | 9 << 28,
        0,
        (levels - 1) << 4,
        0,
        0,
    ]
}

/// An `8_8_8_8_SNORM` image (`GFX10_FORMAT_8_8_8_8_SNORM`, 57, `gfx10-rsrc.json:62`) is a word a
/// texel of four signed normalised bytes, read with its channels where they lie: Grand Theft Auto
/// V's in-world compute pass fetches a 256 x 256 one in `64KB_R_X`. Selects that move its channels
/// are not read.
#[test]
fn a_signed_normalised_four_byte_image_is_a_word() {
    let words = descriptor(57, 256, 27, (1, 0), IDENTITY);
    let format =
        super::dispatch_format(&super::decode_image_descriptor(words), words).expect("read");
    assert_eq!(format, super::TexelFormat::Rgba8Snorm);
    assert_eq!(
        (format.bytes(), format.words(), format.is_integer()),
        (4, 1, false)
    );
    assert!(!super::reorders_packed_channels(format, words[3]));
    let swapped = descriptor(57, 256, 27, (1, 0), 6 | 5 << 3 | 4 << 6 | 7 << 9);
    assert!(super::reorders_packed_channels(format, swapped[3]));
}

/// A fetched image's view of a chain's levels binds the view's base level, where the chain places
/// it, as a draw binds a chain it does not model (D833): Grand Theft Auto V's 256 x 256
/// `8_8_8_8_SNORM` image in `64KB_R_X` with six levels.
///
/// A 64 KiB block of four-byte elements is 128 x 128 of them (`addrlib2.cpp:1698-1709`), and a
/// thin mip tail is the block with its width halved, 64 x 128 (`addrlib2.cpp:1844`), holding up to
/// twelve levels (`gfx10addrlib.cpp:2151`), so the tail starts at level 2, 64 x 64 with four levels
/// to the end (`gfx10addrlib.cpp:3935`, `gfx10addrlib.h:529-540`). The chain lays the tail first,
/// one block, then level 1, 128 x 128 texels, at 64 KiB, and level 0 at 64 KiB plus level 1's 64
/// KiB (`gfx10addrlib.cpp:3979-3989`). Within level 0 each texel lies where addrlib's own `64KB_R_X`
/// equation at four bytes places it ([`crate::swizzle::equation`], read off addrlib by
/// `tools/addrlib/equation`), its pitch 256, two blocks across, with no pipe-bank XOR.
#[test]
fn a_fetched_view_of_several_levels_binds_its_base_level() {
    const LEVEL_ZERO: usize = 0x2_0000;
    const SIDE: u32 = 256;
    let equation = crate::swizzle::equation(crate::registers::SwizzleMode::Tiled64KbRX, 4)
        .expect("addrlib's 64KB_R_X at four bytes");
    // Every texel its own bits; every byte outside level 0 one no texel of it holds.
    let linear: Vec<u32> = (0..SIDE * SIDE)
        .map(|i| i.wrapping_mul(0x9E37_79B1) | 1)
        .collect();
    let bytes: Vec<u8> = linear.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mut chain = vec![0_u8; LEVEL_ZERO + (SIDE * SIDE * 4) as usize];
    equation
        .tile(&bytes, &mut chain[LEVEL_ZERO..], (SIDE, SIDE), (SIDE, 0))
        .expect("level 0 tiled");
    let memory = Memory(chain);
    let words = descriptor(57, SIDE, 27, (6, 5), IDENTITY);
    let source = TextureSource {
        user_data: Some(0),
        ..TextureSource::default()
    };
    let (texels, span) = super::fetched_image(
        &state(words),
        &source,
        &memory,
        &mut super::DetiledImages::default(),
        None,
    )
    .expect("bound");
    assert_eq!(
        (texels.format, texels.width, texels.height),
        (super::TexelFormat::Rgba8Snorm, SIDE, SIDE)
    );
    assert_eq!(span.0, IMAGE_AT + LEVEL_ZERO as u64);
    assert_eq!(&texels.texels[..], linear.as_slice());
}

/// A store to a view of several levels stays refused: a store writes one level, which such a view
/// does not name.
#[test]
fn a_stored_view_of_several_levels_is_refused() {
    let words = descriptor(57, 256, 27, (6, 5), IDENTITY);
    let source = TextureSource {
        user_data: Some(0),
        ..TextureSource::default()
    };
    let memory = Memory(vec![0; 0x6_0000]);
    let refused = super::stored_image(
        &state(words),
        &source,
        &memory,
        &mut super::DetiledImages::default(),
        None,
    )
    .expect_err("refused");
    assert!(refused.contains("more than one level"), "{refused}");
}

/// What a store to an image writes is bounded by its descriptor before the dispatch runs (D989):
/// the level it views, where the descriptor places it - a 256 x 256 four-byte image in `64KB_R_X`
/// is two blocks across and two down, 256 KiB from its base.
#[test]
fn a_stored_image_writes_the_level_its_descriptor_places() {
    let words = descriptor(57, 256, 27, (1, 0), IDENTITY);
    let source = TextureSource {
        user_data: Some(0),
        ..TextureSource::default()
    };
    let memory = Memory(vec![0; 0x4_0000]);
    assert_eq!(
        super::stored_image_spans(&state(words), &source, &memory),
        Some(vec![(IMAGE_AT, 0x4_0000)])
    );
    // A descriptor that does not place the image bounds nothing.
    let several = descriptor(57, 256, 27, (6, 5), IDENTITY);
    assert_eq!(
        super::stored_image_spans(&state(several), &source, &memory),
        None
    );
}

/// A queued dispatch storing to an image is bounded once a translation of its program has named
/// where the image's descriptor comes from, kept by the program's address and bytes (D989): before
/// its first preparation it is not bounded, after it the image's level is what it writes.
#[test]
fn a_queued_image_store_is_bounded_once_its_program_is_translated() {
    use orbistoun_translate::{Fidelity, Strategy};
    const PROGRAM: u64 = 0x1_0000;
    struct Regions(Vec<(u64, Vec<u8>)>);
    impl super::GuestMemory for Regions {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            self.0.iter().find_map(|(base, bytes)| {
                let start = usize::try_from(address.checked_sub(*base)?).ok()?;
                bytes.get(start..start.checked_add(length)?)
            })
        }
    }
    // image_store v[2:5], v[0:1], s[8:15] dmask:0xf dim:SQ_RSRC_IMG_2D unorm; s_endpgm.
    let mut program: Vec<u8> = [0xf020_1f08_u32, 0x0002_0200, 0xbf81_0000, 0xbf9f_0000]
        .map(u32::to_le_bytes)
        .concat();
    program.resize(super::MAX_SHADER_BYTES, 0);
    let memory = Regions(vec![(PROGRAM, program), (IMAGE_AT, vec![0; 0x4_0000])]);
    let mut user_data = vec![Some(0); 8];
    user_data.extend(descriptor(56, 256, 27, (1, 0), IDENTITY).map(Some));
    let state = DispatchState {
        program: PROGRAM,
        user_data,
        inputs: ComputeInputs {
            thread_id_components: 2,
            threads: [8, 8, 1],
            ..ComputeInputs::default()
        },
        width: Width::Wave64,
        dx10_clamp: None,
        local_words: None,
        groups: [1, 1, 1],
        vertex_streamout: false,
    };
    let mut pipeline = super::Pipeline::new(Strategy::Predicated {
        fidelity: Fidelity::Wavefront,
        width: Width::Wave64,
    })
    .expect("a pipeline");
    assert_eq!(
        pipeline.dispatch_write_spans(&state, &memory),
        None,
        "no translation has named the image's source yet"
    );
    pipeline
        .prepare_dispatch(&state, &memory)
        .expect("the store prepares");
    assert_eq!(
        pipeline.dispatch_write_spans(&state, &memory),
        Some(vec![(IMAGE_AT, 0x4_0000)])
    );
}
