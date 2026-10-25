//! A dispatch's fetched image in a block-compressed format, decoded to the texels a dispatch binds.
//!
//! A dispatch binds every image as linear texels of a [`TexelFormat`], none of them blocks. A
//! block-compressed image it samples is decoded on the CPU, as a draw's is where the device cannot
//! sample its blocks (D923): its blocks placed as addrlib places a block-compressed surface's
//! elements, four by four texels each (`addrelemlib.cpp:1451-1455` for `BC1`'s eight bytes), then
//! widened by [`crate::formats::decode`]. Only a format widened to `8_8_8_8_UNORM` texels and read
//! through its selects as they are is decoded here, since that is the one dispatch format its texels
//! are bound as unchanged.
//!
//! A view of several levels is bound as its base level alone, as a draw binds a block-compressed
//! texture (D833): a sample of a coarser level reads the base level's texels.

use super::{
    DETILED_BYTES_MOST, DETILED_KEPT, DetiledImages, DispatchSurface, DispatchTexels, GuestMemory,
    IMAGE_TYPE_2D, SwizzleMode, TexelFormat, TextureSource, decode_image_descriptor,
    describe_dispatch_image, destination_selects, dispatch_descriptor_words, dispatch_sampling,
};
use crate::formats::{HostChannel, Layout, Sampled, Swizzle, Widened};

/// A fetched image's texels, and the first byte and length of what they were read from.
type Fetched = (DispatchTexels, (u64, u64));

/// A level's decoded texels, its extent, and the first byte and length of what they were read from.
type DecodedLevel = (std::sync::Arc<[u32]>, (u32, u32), (u64, u64));

/// The texels a dispatch fetches from a block-compressed image, and where they were read. `None`
/// for an image in any other format, or a block format not decoded here, which the texel readers
/// take - and refuse by its format.
pub(super) fn fetched_blocks(
    state: &crate::dispatch::DispatchState,
    source: &TextureSource,
    memory: &impl GuestMemory,
    detiled: &mut DetiledImages,
    resident: Option<&mut Option<crate::resident::Use>>,
) -> Option<Result<Fetched, String>> {
    let words = dispatch_descriptor_words(state, source, memory).ok()?;
    let sampled = decoded_blocks(
        decode_image_descriptor(words).format,
        destination_selects(words[3]),
    )?;
    Some(read_blocks(
        state,
        (source, words, sampled),
        memory,
        (detiled, resident),
    ))
}

/// [`fetched_blocks`] for an image whose descriptor's `words` name a block format decoded as
/// `sampled`: refused by name where it is not a 2D image read as one, or its base level is not
/// read.
fn read_blocks(
    state: &crate::dispatch::DispatchState,
    (source, words, sampled): (&TextureSource, [u32; 8], Sampled),
    memory: &impl GuestMemory,
    (detiled, resident): (
        &mut DetiledImages,
        Option<&mut Option<crate::resident::Use>>,
    ),
) -> Result<Fetched, String> {
    let descriptor = decode_image_descriptor(words);
    let refuse = |why: &str| {
        format!(
            "{why}: format {}",
            describe_dispatch_image(&descriptor, words)
        )
    };
    if words[3] >> 28 != IMAGE_TYPE_2D || source.volume || source.cube {
        return Err(refuse(
            "a block-compressed dispatch image that is not a 2D image read as one",
        ));
    }
    let (texels, (width, height), span) = base_level_texels(
        &descriptor,
        sampled,
        memory,
        (detiled, resident, source.layered),
    )
    .map_err(refuse)?;
    Ok((
        DispatchTexels {
            texels,
            format: TexelFormat::Rgba8,
            width,
            height,
            depth: None,
            layers: source.layered.then_some(1),
            sampling: dispatch_sampling(state, source, memory)?,
            cube: false,
        },
        span,
    ))
}

/// What `format` is decoded as, where it is a block format widened to `8_8_8_8_UNORM` texels
/// ([`crate::formats::sampled`]) that `selects` read as they are: `X` to `W` the widened texel's red
/// to alpha. `None` for any other format or selects.
fn decoded_blocks(format: u32, selects: [u32; 4]) -> Option<Sampled> {
    let sampled = crate::formats::sampled(format).ok()?;
    let as_they_are = [
        HostChannel::R,
        HostChannel::G,
        HostChannel::B,
        HostChannel::A,
    ]
    .map(Swizzle::Channel);
    (matches!(sampled.layout, Layout::Blocks { .. })
        && sampled.widened == Widened::Rgba8Unorm
        && sampled.view(selects, false) == Some(as_they_are))
    .then_some(sampled)
}

/// The view's base level of a block-compressed image, decoded: its texels, its extent, and the
/// bytes it was read from. Read as a draw's blocks are (`read_block_texture`): a standard swizzle,
/// uncompressed, at a level whose blocks are its chain's halved elements. Where `resident` is given,
/// its place among the images the device keeps goes there (D985), and one taken from the device is
/// neither read nor decoded: its texels come back empty.
fn base_level_texels(
    descriptor: &crate::registers::ImageDescriptor,
    sampled: Sampled,
    memory: &impl GuestMemory,
    (detiled, resident, layered): (
        &mut DetiledImages,
        Option<&mut Option<crate::resident::Use>>,
        bool,
    ),
) -> Result<DecodedLevel, &'static str> {
    if !matches!(
        descriptor.tiling,
        SwizzleMode::Tiled64KbS | SwizzleMode::Tiled4KbS | SwizzleMode::Tiled256BS
    ) {
        return Err("a block-compressed dispatch image in a tiling its blocks are not read in");
    }
    if descriptor.compression.is_some() {
        return Err("a block-compressed dispatch image compressed further (DCC)");
    }
    let layout = crate::tiling::SurfaceLayout::of(descriptor.tiling)
        .and_then(|layout| layout.at_bytes_per_texel(sampled.layout.element_bytes()))
        .ok_or(
            "a block-compressed dispatch image whose blocks' size has no layout in its tiling",
        )?;
    let level = if descriptor.levels <= 1 {
        0
    } else {
        descriptor.base_level
    };
    let (width, height) = (
        (descriptor.width >> level).max(1),
        (descriptor.height >> level).max(1),
    );
    let surface = crate::registers::chain_level(
        (descriptor.base, 0),
        (descriptor.width.div_ceil(4), descriptor.height.div_ceil(4)),
        (descriptor.levels, level),
        layout,
    )
    .ok_or("a block-compressed dispatch image whose base level is not placed in its chain")?;
    if (surface.width, surface.height) != (width.div_ceil(4), height.div_ceil(4)) {
        return Err(
            "a block-compressed dispatch image whose base level is not its chain's halved blocks (an odd extent)",
        );
    }
    let span = surface.words() * 4;
    if let Some(slot) = resident {
        // Bound as an array of one where the program reads it as an array: another image.
        let view = (format!("{surface:?}"), descriptor.format, "blocks", layered);
        let bytes = u64::from(width) * u64::from(height) * 4;
        *slot = super::place_resident((surface.base, span as u64), &view, bytes, memory);
        if slot.is_some_and(|used| !used.fresh) {
            return Ok((
                Vec::new().into(),
                (width, height),
                (surface.base, span as u64),
            ));
        }
    }
    let spanned = memory
        .read(surface.base, span)
        .ok_or("a block-compressed dispatch image not in guest memory it can read")?;
    let texels = detiled
        .decoded(
            DispatchSurface::Tiled(surface),
            spanned,
            descriptor.format,
            || {
                let tiled: Vec<u32> = spanned
                    .chunks_exact(4)
                    .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                    .collect();
                let blocks: Vec<u8> = surface
                    .detile_mapped(&tiled, |word| word)
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect();
                crate::formats::decode(&sampled, &blocks, (width, height))
            },
        )
        .ok_or("a block-compressed dispatch image whose blocks do not decode")?;
    Ok((texels, (width, height), (surface.base, span as u64)))
}

impl DetiledImages {
    /// `spanned`, `surface`'s bytes, decoded by `decode` from blocks of guest format `format`: the
    /// texels kept for them where they are kept, else decoded now and kept. A decode is kept under
    /// its bytes followed by its format's code, which no detile's bytes match, being longer, so
    /// the texels a detile and a decode of the same bytes give are never taken for each other.
    fn decoded(
        &mut self,
        surface: DispatchSurface,
        spanned: &[u8],
        format: u32,
        decode: impl FnOnce() -> Option<Vec<u32>>,
    ) -> Option<std::sync::Arc<[u32]>> {
        let mut keyed = Vec::with_capacity(spanned.len() + 4);
        keyed.extend_from_slice(spanned);
        keyed.extend_from_slice(&format.to_le_bytes());
        if let Some(at) = self
            .0
            .iter()
            .position(|(kept, bytes, _)| *kept == surface && *bytes == keyed)
        {
            // The most recent last, so the oldest goes first.
            let entry = self.0.remove(at);
            let texels = entry.2.clone();
            self.0.push(entry);
            return Some(texels);
        }
        let started = std::time::Instant::now();
        let texels: std::sync::Arc<[u32]> = decode()?.into();
        crate::detiled::note(
            &format!("format {format} decoded for a dispatch"),
            spanned.len() as u64,
            started.elapsed(),
        );
        let adding = keyed.len() + texels.len() * 4;
        while !self.0.is_empty()
            && (self.0.len() >= DETILED_KEPT || self.bytes() + adding > DETILED_BYTES_MOST)
        {
            self.0.remove(0);
        }
        self.0.push((surface, keyed, texels.clone()));
        Some(texels)
    }
}

#[cfg(test)]
mod tests {
    use crate::dispatch::DispatchState;
    use orbistoun_translate::Width;
    use orbistoun_translate::wavefront::{ComputeInputs, TextureSource};

    /// Where Grand Theft Auto V's image lies.
    const BASE: u64 = 0x15_44e0_0000;

    struct Memory(Vec<u8>);

    impl super::GuestMemory for Memory {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            let start = usize::try_from(address.checked_sub(BASE)?).ok()?;
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

    /// The descriptor of a `size` x `size` image of format `format` at [`BASE`] in swizzle mode
    /// `mode`, viewing levels 0 to `last` of `levels`, through `selects` (`gfx10-rsrc.json`
    /// `SQ_IMG_RSRC_WORD0`-`5`, as `decode_image_descriptor` reads them): a 2D image (`TYPE` 9).
    fn descriptor(
        format: u32,
        size: u32,
        mode: u32,
        (levels, last): (u32, u32),
        selects: u32,
    ) -> [u32; 8] {
        let base = BASE >> 8;
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

    /// `X`, `Y`, `Z`, `W`: each channel as it is.
    const IDENTITY: u32 = 4 | 5 << 3 | 6 << 6 | 7 << 9;

    /// A `BC1` block of one colour: both endpoints the same `RGB565` colour, which makes it a
    /// three-colour block, and every index zero, the first endpoint (Khronos Data Format 1.3 section
    /// 21.1).
    fn solid_block(colour: u16) -> [u8; 8] {
        let [low, high] = colour.to_le_bytes();
        [low, high, low, high, 0, 0, 0, 0]
    }

    /// An `RGB565` colour as eight-bit channels and alpha one, each field's bits moved to the top of
    /// its byte and its top bits repeated below them (Data Format section 21.1, the endpoints).
    fn expected_texel(colour: u16) -> u32 {
        let colour = u32::from(colour);
        let (red, green, blue) = (colour >> 11, colour >> 5 & 0x3F, colour & 0x1F);
        let (red, green, blue) = (
            red << 3 | red >> 2,
            green << 2 | green >> 4,
            blue << 3 | blue >> 2,
        );
        red | green << 8 | blue << 16 | 0xFF << 24
    }

    /// The colour of block (`x`, `y`): every block's its own.
    fn colour_of((x, y): (u32, u32)) -> u16 {
        u16::try_from(y * 256 + x).expect("256 x 256 blocks")
    }

    /// A fetched `BC1_UNORM` image (`GFX10_FORMAT_BC1_UNORM`, 169, `gfx10-rsrc.json:123`) is its
    /// view's base level decoded to `8_8_8_8_UNORM` texels: Grand Theft Auto V's 1024 x 1024 image in
    /// `64KB_S` with eleven levels, read by an in-world compute pass.
    ///
    /// addrlib lays a `BC1` surface out in elements of four by four texels and eight bytes
    /// (`addrelemlib.cpp:1451-1455`), so level 0 is 256 x 256 elements and the chain eleven levels.
    /// A 64 KiB block of eight-byte elements is 128 x 64 of them (`addrlib2.cpp:1698-1709`), and a
    /// thin mip tail is the block with its width halved, 64 x 64 (`addrlib2.cpp:1844`), holding up
    /// to twelve levels (`gfx10addrlib.cpp:2151`), so the tail starts at level 2, 64 x 64 elements
    /// with nine levels to the end (`gfx10addrlib.cpp:3935`, `gfx10addrlib.h:529-540`). The chain
    /// lays the tail first, one block, then each level from the coarsest: level 1, 128 x 128
    /// elements, at 64 KiB, and level 0 at 64 KiB plus level 1's 128 KiB
    /// (`gfx10addrlib.cpp:3979-3989`). Within level 0 each element lies where
    /// addrlib's own `64KB_S` equation at eight bytes places it ([`crate::swizzle::equation`], read
    /// off addrlib by `tools/addrlib/equation`), its pitch 256 elements, a whole number of blocks.
    #[test]
    fn a_bc1_image_is_its_base_level_decoded() {
        const LEVEL_ZERO: usize = 0x1_0000 + 0x2_0000;
        const ELEMENTS: u32 = 256;
        let equation = crate::swizzle::equation(crate::registers::SwizzleMode::Tiled64KbS, 8)
            .expect("addrlib's 64KB_S at eight bytes");
        // Every byte outside level 0 one no block of it holds, so a read of another level shows.
        let mut chain = vec![0xA5_u8; LEVEL_ZERO + 256 * 256 * 8];
        let linear: Vec<u8> = (0..ELEMENTS)
            .flat_map(|y| (0..ELEMENTS).flat_map(move |x| solid_block(colour_of((x, y)))))
            .collect();
        equation
            .tile(
                &linear,
                &mut chain[LEVEL_ZERO..],
                (ELEMENTS, ELEMENTS),
                (ELEMENTS, 0),
            )
            .expect("level 0 tiled");
        let memory = Memory(chain);
        let words = descriptor(169, 1024, 9, (11, 10), IDENTITY);
        let source = TextureSource {
            user_data: Some(0),
            ..TextureSource::default()
        };
        let (texels, span) = super::fetched_blocks(
            &state(words),
            &source,
            &memory,
            &mut super::DetiledImages::default(),
            None,
        )
        .expect("a block-compressed image")
        .expect("read");
        assert_eq!(
            (texels.format, texels.width, texels.height, texels.layers),
            (super::super::TexelFormat::Rgba8, 1024, 1024, None)
        );
        assert_eq!(span, (BASE + LEVEL_ZERO as u64, 256 * 256 * 8));
        assert_eq!(texels.texels.len(), 1024 * 1024);
        for (at, &texel) in texels.texels.iter().enumerate() {
            let (x, y) = ((at % 1024) as u32, (at / 1024) as u32);
            let want = expected_texel(colour_of((x / 4, y / 4)));
            assert_eq!(texel, want, "texel ({x}, {y})");
        }
    }

    /// A block-compressed image whose selects move its channels, or one with channels the host's
    /// widened texel answers differently, is not decoded here: `BC1` read `Z`, `Y`, `X`, `W`, and a
    /// one-channel `BC4_UNORM` (`gfx10-rsrc.json`, value 175) read through all four.
    #[test]
    fn only_blocks_read_as_they_are_are_decoded() {
        let memory = Memory(vec![0; 0x1000]);
        let source = TextureSource {
            user_data: Some(0),
            ..TextureSource::default()
        };
        for words in [
            descriptor(169, 4, 9, (1, 0), 6 | 5 << 3 | 4 << 6 | 7 << 9),
            descriptor(175, 4, 9, (1, 0), IDENTITY),
        ] {
            assert!(
                super::fetched_blocks(
                    &state(words),
                    &source,
                    &memory,
                    &mut super::DetiledImages::default(),
                    None,
                )
                .is_none()
            );
        }
    }

    /// A `BC1` image in a tiling block-compressed images are not read in is refused by name, as the
    /// block-compressed image it is: `64KB_R_X` (swizzle mode 27).
    #[test]
    fn a_bc1_image_in_another_tiling_is_refused_by_name() {
        let memory = Memory(vec![0; 0x1_0000]);
        let source = TextureSource {
            user_data: Some(0),
            ..TextureSource::default()
        };
        let refused = super::fetched_blocks(
            &state(descriptor(169, 4, 27, (1, 0), IDENTITY)),
            &source,
            &memory,
            &mut super::DetiledImages::default(),
            None,
        )
        .expect("a block-compressed image")
        .expect_err("refused");
        assert!(
            refused.contains("tiling") && refused.contains("169"),
            "{refused}"
        );
    }
}
