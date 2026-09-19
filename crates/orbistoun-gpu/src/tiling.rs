//! Detiling a guest's surfaces - turning a tiled render target or texture back into a linear image.
//!
//! A guest's colour targets and textures are **tiled**: a texel `(x, y)` is not at the linear byte
//! `(y * width + x) * bpp`, but at a swizzled offset the hardware computes. To read a guest's
//! surface (to upload a texture, or to diff a captured framebuffer) the swizzle has to be reproduced
//! exactly. A wrong swizzle is the failure this project is least able to see: it passes every test
//! built from linear data and corrupts every genuinely tiled surface (principle 3). So the swizzle
//! here is not guessed - it is anchored to a hardware measurement.
//!
//! # Provenance: one measured byte, and a swizzle software models agree on beyond it
//!
//! **One texel was read back from hardware, and no more.** obSCEne's draw oracle
//! `166-agc/primitive-draw` (sweep `20260916-223136`) rendered one pixel into a `64KB_R_X` 32-bpp
//! colour target and read a single address back: `color-idx 0x43f`, tiled byte **4348**. That is the
//! one hardware fact. Its texel is fixed by the capture's own geometry, not assumed: the draw is a
//! point (`VGT_GS_OUT_PRIM_TYPE` = POINTLIST), its vertex is NDC `(-0.5, -0.5)`, and the viewport
//! transform in the same stream (`PA_CL_VPORT_*` scale/offset = 32) carries it to screen corner
//! `(16.0, 16.0)`, which a down-left rasteriser resolves to texel **`(15, 15)`**.
//!
//! **Everything past that one byte is agreement among independent software tiler models, not a
//! readback.** The equation below is derived to fit `(15, 15)` → 4348, and it agrees entry for entry
//! with obSCEne's `agc_detile_pixel` (a `static inline` in `oops-sdk` that *computes* the offset). So
//! a second point like `(32, 21)` → 2640 and the mapping being bijective over all 16,384 texels of
//! the 128×128 block are things those models compute alike - real corroboration that the equation is
//! right, but not a hardware measurement. obSCEne's `166-agc/tiling-swizzle` check submits no draw;
//! its rows are that function's outputs, and where a resolution (`-db54`) called them "measured on
//! hardware" it was quoting a host build's computation (worklog 674 carried that error in; this
//! restates it). The honest ceiling is one texel read back and a swizzle two independent
//! implementations (this one and obSCEne's) agree on across one block - which is why
//! `SINGLE_BLOCK_EXTENT` stays at 128, where no readback has gone.
//! The first real multi-texel readback on offer is the triangle record (512 drawn pixels, `-f432`),
//! not yet in the tree.
//!
//! # Why only `64KB_R_X` at 32 bpp, and not `4KB_S` or the block-compressed formats
//!
//! Guest **colour targets** are `64KB_R_X` 32-bpp (obSCEne measured `cb0-tiling-mode 0x1b` on every
//! draw), which is the mode a render pass reads back, and the only one a hardware measurement could
//! reach: obSCEne established (`-6e0f`) that **no reachable AGC API exposes surface detiling on
//! retail** - a compute `image_store` into a tiled surface faults the GPU pipe - so the swizzle was
//! obtainable only by *rendering* into such a surface and reading it back, which works for a render
//! target and not for a texture. `4KB_S` (a texture tiling) and the block-compressed layouts
//! therefore have no measurement, and this project does not derive a swizzle from a manual: a wrong
//! one passes every round-trip and linear test and corrupts only genuinely tiled surfaces (principle
//! 3), so it is refused by name rather than guessed. They wait on a measurement, and are not needed
//! before a backend samples textures at all. Block-compressed data needs no
//! decode regardless - Vulkan consumes it natively (roadmap G15).

use crate::registers::{
    ImageDescriptor, RegisterWrite, SwizzleMode, colour_swizzle_mode_at, colour_target_at,
};

/// The byte offset of texel `(x, y)` within a 64 KiB `R_X` tile, for a four-byte (`R8G8B8A8`-class)
/// element - the `64KB_R_X` swizzle for a 32-bpp surface.
///
/// This is the offset **within one 64 KiB block**, which is the whole address for a surface that
/// fits in one block (up to 128x128 texels at 32 bpp - the measured `-a1f7` target is 64x64). A
/// larger surface also needs the block-level tiling and the pipe/bank swizzle (`pipeBankXor`), which
/// [`detile_64kb_rx_bpp4`] refuses rather than models; the measured surface has `pipeBankXor` zero.
///
/// The pattern is a set of `XOR`ed bit selections - each guest coordinate bit lands in a fixed output
/// bit, the shape a hardware address equation takes. It is fitted to the one texel read back on
/// hardware, `(15, 15)` → byte 4348, and reproduces what obSCEne's own tiler *computes* for other
/// texels - e.g. `(32, 21)` → 2640 - agreeing with it as a bijection across all 16,384 texels of the
/// 128x128 macro-tile. That is software-model agreement, not a second readback (see the module docs).
#[must_use]
pub fn tiled_byte_offset_64kb_rx_bpp4(x: u32, y: u32) -> u32 {
    let mut offset = 0;
    // The row's contribution: y's low bits fan out across the block.
    offset ^= (y << 4) & 0x0070;
    offset ^= (y << 5) & 0x0f00;
    offset ^= (y << 9) & 0x1000;
    offset ^= (y << 8) & 0x4000;
    // The column's contribution.
    offset ^= (x << 2) & 0x000c;
    offset ^= (x << 5) & 0x0380;
    offset ^= (x << 4) & 0x0400;
    offset ^= (x << 6) & 0x0800;
    offset ^= (x << 9) & 0xa000;
    offset
}

/// The widest surface the single-block swizzle covers, per dimension, at 32 bpp.
///
/// A 64 KiB block holds 128x128 four-byte texels; beyond that a surface spans multiple blocks, which
/// needs the block-level tiling this does not model.
const SINGLE_BLOCK_EXTENT: u32 = 128;

/// Why a surface could not be detiled, stated rather than papered over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetileError {
    /// The surface is larger than one 64 KiB block in some dimension, so it needs block-level tiling
    /// and a `pipeBankXor` that are not modelled yet (only the intra-block swizzle is measured).
    SurfaceExceedsBlock {
        /// Width in texels, as asked.
        width: u32,
        /// Height in texels, as asked.
        height: u32,
    },
    /// The tiled words given do not reach the highest offset the swizzle addresses for this surface.
    TiledDataTooShort {
        /// Words the swizzle needs to reach every texel.
        needed_words: usize,
        /// Words actually supplied.
        got_words: usize,
    },
    /// The surface's format is not one the 32-bpp swizzle applies to - the element is a different
    /// size, or a size this does not model - so detiling it would read across or within elements.
    UnsupportedFormat {
        /// The GFX10 `IMG_FORMAT` code the descriptor carried.
        format: u32,
    },
}

impl std::fmt::Display for DetileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SurfaceExceedsBlock { width, height } => write!(
                f,
                "surface {width}x{height} exceeds one 64KB block ({SINGLE_BLOCK_EXTENT} per side); block-level tiling is not measured"
            ),
            Self::TiledDataTooShort {
                needed_words,
                got_words,
            } => write!(
                f,
                "tiled data is {got_words} words, swizzle needs {needed_words} to reach every texel"
            ),
            Self::UnsupportedFormat { format } => write!(
                f,
                "image format {format} is not a 32-bpp format the measured swizzle applies to"
            ),
        }
    }
}

impl std::error::Error for DetileError {}

/// Detile a 32-bpp `64KB_R_X` surface into a linear, row-major image of `width * height` texels.
///
/// `tiled` is the surface's own words in guest memory (the caller slices at the descriptor's base).
/// The result at index `y * width + x` is the texel the swizzle placed at
/// [`tiled_byte_offset_64kb_rx_bpp4`]`(x, y)`. Refuses, rather than guesses, a surface beyond one
/// block or tiled data too short to cover the swizzle (see [`DetileError`]).
///
/// # Errors
///
/// [`DetileError::SurfaceExceedsBlock`] if either dimension is over 128; [`DetileError::
/// TiledDataTooShort`] if `tiled` does not reach the highest swizzled word this surface addresses.
pub fn detile_64kb_rx_bpp4(
    tiled: &[u32],
    width: u32,
    height: u32,
) -> Result<Vec<u32>, DetileError> {
    if width > SINGLE_BLOCK_EXTENT || height > SINGLE_BLOCK_EXTENT {
        return Err(DetileError::SurfaceExceedsBlock { width, height });
    }

    // The highest word the swizzle reaches over this surface decides whether the data is long enough,
    // checked once here so the per-texel reads below are all in bounds.
    let mut needed_words = 0usize;
    for y in 0..height {
        for x in 0..width {
            let word = tiled_byte_offset_64kb_rx_bpp4(x, y) as usize / 4;
            needed_words = needed_words.max(word + 1);
        }
    }
    if tiled.len() < needed_words {
        return Err(DetileError::TiledDataTooShort {
            needed_words,
            got_words: tiled.len(),
        });
    }

    let mut linear = vec![0u32; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let word = tiled_byte_offset_64kb_rx_bpp4(x, y) as usize / 4;
            linear[(y * width + x) as usize] = tiled[word];
        }
    }
    Ok(linear)
}

/// Detile the surface an [`ImageDescriptor`] names, given the descriptor's own words in guest memory.
///
/// Bytes per texel for a GFX10 `IMG_FORMAT` code, or `None` for one whose element size this does not
/// model.
///
/// The codes are `GFX10_FORMAT_*` (oops-mesa `src/amd/registers/gfx10-rsrc.json`), whose names give
/// the channel bit-widths and which that table lays out in ascending element size: `1..=6` are one
/// byte (a single 8-bit channel), `7..=19` two (a 16-bit channel or `8_8`), `20..=61` four (`32`,
/// `16_16`, `10_11_11`, `11_11_10`, `10_10_10_2`, `2_10_10_10`, `8_8_8_8`), `62..=71` eight, `72..=74`
/// twelve, `75..=77` sixteen; `128`/`129`/`130` are the `8`/`8_8`/`8_8_8_8` sRGB variants (one, two,
/// four). Every other code - the packed 16-bit, depth, subsampled, FMASK and block-compressed formats
/// past 127 - is left `None` and refused rather than assigned a size from its name, since the detile
/// only needs to tell the 32-bpp case apart from the rest and a wrong size there would mis-read every
/// texel.
fn element_bytes(format: u32) -> Option<u32> {
    match format {
        1..=6 | 128 => Some(1),   // single 8-bit channel, and 8_SRGB
        7..=19 | 129 => Some(2),  // 16-bit or 8_8, and 8_8_SRGB
        20..=61 | 130 => Some(4), // the 32-bpp block, and 8_8_8_8_SRGB
        62..=71 => Some(8),
        72..=74 => Some(12),
        75..=77 => Some(16),
        _ => None,
    }
}

/// The element size the measured `64KB_R_X` swizzle is defined for: four bytes, 32 bpp.
const MEASURED_ELEMENT_BYTES: u32 = 4;

/// A thin wrapper over [`detile_64kb_rx_bpp4`] that takes the extent from the descriptor. It applies
/// the 32-bpp `64KB_R_X` swizzle, so it refuses a format that is not 32 bpp (`element_bytes`); the
/// tiling mode is still the caller's to confirm is `64KB_R_X`, since this reads the pixels directly
/// rather than from a guest window.
///
/// # Errors
///
/// [`DetileError::UnsupportedFormat`] for a non-32-bpp format, else [`detile_64kb_rx_bpp4`]'s
/// refusals.
pub fn detile_image(descriptor: &ImageDescriptor, tiled: &[u32]) -> Result<Vec<u32>, DetileError> {
    if element_bytes(descriptor.format) != Some(MEASURED_ELEMENT_BYTES) {
        return Err(DetileError::UnsupportedFormat {
            format: descriptor.format,
        });
    }
    detile_64kb_rx_bpp4(tiled, descriptor.width, descriptor.height)
}

/// A detiled surface - a colour target or a texture - its pixels laid out linearly, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height` pixels, row-major.
    pub pixels: Vec<u32>,
}

/// Why a surface could not be detiled out of a guest-memory window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
    /// The base or extent was not set - a colour target whose stream wrote neither.
    Incomplete,
    /// The tiling mode is one orbistoun does not detile - linear, or a mode it does not model.
    UnsupportedTiling(SwizzleMode),
    /// The target's base is not inside the guest-memory window given.
    OutsideWindow {
        /// The surface's base address.
        base: u64,
        /// The address the guest slice's first word sits at.
        window_base: u64,
        /// How many words the window holds.
        window_words: usize,
    },
    /// The detile refused: the surface is larger than one block, or the window is too short for it.
    Detile(DetileError),
}

impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Incomplete => write!(f, "the surface's base or extent was not set"),
            Self::UnsupportedTiling(mode) => {
                write!(f, "tiling mode {mode:?} is not one orbistoun detiles")
            }
            Self::OutsideWindow {
                base,
                window_base,
                window_words,
            } => write!(
                f,
                "surface base {base:#x} is outside the guest window at {window_base:#x} ({window_words} words)"
            ),
            Self::Detile(inner) => write!(f, "detile refused: {inner}"),
        }
    }
}

impl std::error::Error for SurfaceError {}

/// Detiles a surface out of a guest-memory window, given its base, extent and tiling mode.
///
/// The shared core of [`detile_colour_target`] and [`detile_texture`]: the base is mapped into
/// `guest` - whose first word sits at address `window_base` - and [`detile_64kb_rx_bpp4`] unswizzles
/// it. It detiles only `64KB_R_X`; any other mode, including linear, is a refusal rather than a wrong
/// swizzle (D010). The element size is 32 bpp; the caller ensures the format matches.
fn detile_surface(
    base: u64,
    width: u32,
    height: u32,
    tiling: SwizzleMode,
    guest: &[u32],
    window_base: u64,
) -> Result<Surface, SurfaceError> {
    if tiling != SwizzleMode::Tiled64KbRX {
        return Err(SurfaceError::UnsupportedTiling(tiling));
    }

    let outside = SurfaceError::OutsideWindow {
        base,
        window_base,
        window_words: guest.len(),
    };
    let offset_bytes = base
        .checked_sub(window_base)
        .ok_or_else(|| outside.clone())?;
    let offset_words = usize::try_from(offset_bytes / 4).map_err(|_| outside.clone())?;
    let words = guest.get(offset_words..).ok_or(outside)?;

    let pixels = detile_64kb_rx_bpp4(words, width, height).map_err(SurfaceError::Detile)?;
    Ok(Surface {
        width,
        height,
        pixels,
    })
}

/// Detiles colour buffer zero out of a guest-memory window, reading its base, extent and tiling mode
/// from the register writes.
///
/// [`colour_target_at`] gives the base and size and [`colour_swizzle_mode_at`] the tiling; then
/// `detile_surface` maps the base into `guest` (first word at `window_base`) and unswizzles it.
///
/// # Errors
///
/// [`SurfaceError::Incomplete`] when the stream set no base or extent, else `detile_surface`'s
/// refusals (a tiling not modelled, a base outside the window, or a detile refusal).
pub fn detile_colour_target(
    writes: &[RegisterWrite],
    guest: &[u32],
    window_base: u64,
) -> Result<Surface, SurfaceError> {
    let target = colour_target_at(writes).ok_or(SurfaceError::Incomplete)?;
    let mode = colour_swizzle_mode_at(writes).ok_or(SurfaceError::Incomplete)?;
    detile_surface(
        target.base,
        target.width,
        target.height,
        mode,
        guest,
        window_base,
    )
}

/// Detiles a texture out of a guest-memory window, from its decoded descriptor.
///
/// The texture analog of [`detile_colour_target`]: an [`ImageDescriptor`] already carries the base,
/// extent, format and tiling (`crate::registers::decode_image_descriptor`). It applies the 32-bpp
/// `64KB_R_X` swizzle, so it refuses a descriptor whose format is not 32 bpp (`element_bytes`)
/// before touching the window - a different element size is a different swizzle, not this one - then
/// hands the rest to `detile_surface`.
///
/// # Errors
///
/// [`SurfaceError::Detile`]`(`[`DetileError::UnsupportedFormat`]`)` for a non-32-bpp format;
/// otherwise `UnsupportedTiling` unless the descriptor's tiling is `64KB_R_X`, else the window and
/// detile refusals. It never returns `Incomplete` - a descriptor always carries base and extent.
pub fn detile_texture(
    descriptor: &ImageDescriptor,
    guest: &[u32],
    window_base: u64,
) -> Result<Surface, SurfaceError> {
    if element_bytes(descriptor.format) != Some(MEASURED_ELEMENT_BYTES) {
        return Err(SurfaceError::Detile(DetileError::UnsupportedFormat {
            format: descriptor.format,
        }));
    }
    detile_surface(
        descriptor.base,
        descriptor.width,
        descriptor.height,
        descriptor.tiling,
        guest,
        window_base,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DetileError, SurfaceError, detile_64kb_rx_bpp4, detile_colour_target, detile_texture,
        tiled_byte_offset_64kb_rx_bpp4 as offset,
    };
    use crate::registers::{ImageDescriptor, RegisterWrite, SwizzleMode};

    /// Register writes setting colour buffer zero's base (256-byte units), extent and tiling.
    fn target_writes(base_256b: u32, attrib2: u32, attrib3: u32) -> Vec<RegisterWrite> {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        // CB_COLOR0_BASE 0xA318, CB_COLOR0_ATTRIB2 0xA3B0, CB_COLOR0_ATTRIB3 0xA3B8.
        vec![
            write(0xA318, base_256b),
            write(0xA3B0, attrib2),
            write(0xA3B8, attrib3),
        ]
    }

    /// **The bridge reads base, extent and tiling from the stream and detiles the surface, offset
    /// into the window and all.**
    ///
    /// A 64x64 `64KB_R_X` target based part-way into the guest window: the measured pixel at tiled
    /// byte 4348 must come back at linear index `15*64+15`, with the base-to-window offset applied.
    #[test]
    fn detile_colour_target_reads_the_stream_and_places_the_measured_pixel() {
        const RED: u32 = 0xff00_00ff;
        // Base 0x2000e0000; window starts 0x400 bytes (256 words) earlier, so the surface is at
        // word offset 256 in the guest slice.
        let base = 0x2_000e_0000u64;
        let window_base = base - 0x400;
        let writes = target_writes(0x0200_0e00, 0x000f_c03f, 0x08c6_c000);
        let mut guest = vec![0u32; 256 + 4096];
        guest[256 + 4348 / 4] = RED; // the measured tiled byte, in the surface at offset 256

        let surface = detile_colour_target(&writes, &guest, window_base).expect("detiles");
        assert_eq!((surface.width, surface.height), (64, 64));
        assert_eq!(
            surface.pixels[(15 * 64 + 15) as usize],
            RED,
            "texel (15,15)"
        );
        assert_eq!(surface.pixels.iter().filter(|&&p| p == RED).count(), 1);
    }

    /// **A stream missing the base or the extent is refused as incomplete.**
    #[test]
    fn detile_colour_target_refuses_incomplete_state() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        // Extent and tiling set, but no base.
        let writes = vec![write(0xA3B0, 0x000f_c03f), write(0xA3B8, 0x08c6_c000)];
        assert_eq!(
            detile_colour_target(&writes, &[0u32; 4096], 0),
            Err(SurfaceError::Incomplete)
        );
    }

    /// **A tiling mode orbistoun does not detile is refused by name, not read with the wrong swizzle.**
    #[test]
    fn detile_colour_target_refuses_a_mode_it_does_not_model() {
        // ATTRIB3 = 0 -> COLOR_SW_MODE 0 -> linear, which the detile does not handle.
        let writes = target_writes(0x0200_0e00, 0x000f_c03f, 0x0000_0000);
        assert_eq!(
            detile_colour_target(&writes, &[0u32; 4096], 0x2_000e_0000),
            Err(SurfaceError::UnsupportedTiling(SwizzleMode::Linear))
        );
    }

    /// **A base below the window is refused, not read from a negative offset.**
    #[test]
    fn detile_colour_target_refuses_a_base_outside_the_window() {
        let writes = target_writes(0x0200_0e00, 0x000f_c03f, 0x08c6_c000);
        // Window starts *after* the base, so the base is not in it.
        let window_base = 0x2_000e_0000u64 + 0x1000;
        match detile_colour_target(&writes, &[0u32; 4096], window_base) {
            Err(SurfaceError::OutsideWindow { base, .. }) => assert_eq!(base, 0x2_000e_0000),
            other => panic!("expected an outside-window refusal, got {other:?}"),
        }
    }

    /// **A tiled texture detiles from its descriptor, the texture analog of the colour-target case.**
    ///
    /// A 64x64 `64KB_R_X` T# based part-way into the window: the measured pixel at byte 4348 comes
    /// back at linear index `15*64+15`, reading the base, extent and tiling from the descriptor.
    #[test]
    fn detile_texture_reads_the_descriptor_and_places_the_measured_pixel() {
        const RED: u32 = 0xff00_00ff;
        let base = 0x2_000e_0000u64;
        let window_base = base - 0x400;
        let descriptor = ImageDescriptor {
            base,
            width: 64,
            height: 64,
            format: 56, // GFX10_FORMAT_8_8_8_8_UNORM, a 32-bpp format
            tiling: SwizzleMode::Tiled64KbRX,
        };
        let mut guest = vec![0u32; 256 + 4096];
        guest[256 + 4348 / 4] = RED;

        let surface = detile_texture(&descriptor, &guest, window_base).expect("detiles");
        assert_eq!((surface.width, surface.height), (64, 64));
        assert_eq!(
            surface.pixels[(15 * 64 + 15) as usize],
            RED,
            "texel (15,15)"
        );
        assert_eq!(surface.pixels.iter().filter(|&&p| p == RED).count(), 1);
    }

    /// **A linear texture is refused by its mode, not detiled with the tiled swizzle.**
    #[test]
    fn detile_texture_refuses_a_linear_descriptor() {
        let descriptor = ImageDescriptor {
            base: 0x2_000e_0000,
            width: 64,
            height: 64,
            format: 56, // GFX10_FORMAT_8_8_8_8_UNORM, a 32-bpp format
            tiling: SwizzleMode::Linear,
        };
        assert_eq!(
            detile_texture(&descriptor, &[0u32; 4096], 0x2_000e_0000),
            Err(SurfaceError::UnsupportedTiling(SwizzleMode::Linear))
        );
    }

    /// **A non-32-bpp texture is refused, not detiled with the 32-bpp swizzle.**
    ///
    /// The measured swizzle is a four-byte-element mapping, so a `16_16_16_16` (eight-byte) surface
    /// would have every texel read across elements. The negative that makes the format check real:
    /// without it this descriptor would detile as if it were 32 bpp. Refused before the window is even
    /// touched, so a short slice cannot mask it.
    #[test]
    fn detile_texture_refuses_a_non_32_bpp_format() {
        let descriptor = ImageDescriptor {
            base: 0x2_000e_0000,
            width: 64,
            height: 64,
            format: 71, // GFX10_FORMAT_16_16_16_16_FLOAT, eight bytes per texel
            tiling: SwizzleMode::Tiled64KbRX,
        };
        assert_eq!(
            detile_texture(&descriptor, &[0u32; 4096], 0x2_000e_0000),
            Err(SurfaceError::Detile(DetileError::UnsupportedFormat {
                format: 71
            })),
        );
    }

    /// **The element-size decode agrees with the cited `GFX10_FORMAT` table at each size boundary.**
    ///
    /// `oops-mesa src/amd/registers/gfx10-rsrc.json` lays the standard formats out in ascending
    /// element size; this pins the boundaries the detile turns on - the 32-bpp block is `20..=61`,
    /// with a two-byte format just below and an eight-byte one just above - and that a
    /// block-compressed code is refused rather than sized.
    #[test]
    fn the_format_element_size_matches_the_cited_table() {
        use super::element_bytes;
        assert_eq!(element_bytes(56), Some(4), "8_8_8_8_UNORM");
        assert_eq!(element_bytes(50), Some(4), "2_10_10_10_UNORM");
        assert_eq!(element_bytes(36), Some(4), "10_11_11_FLOAT");
        assert_eq!(element_bytes(19), Some(2), "8_8_SINT, just below the block");
        assert_eq!(element_bytes(62), Some(8), "32_32_UINT, just above it");
        assert_eq!(element_bytes(71), Some(8), "16_16_16_16_FLOAT");
        assert_eq!(element_bytes(130), Some(4), "8_8_8_8_SRGB");
        assert_eq!(element_bytes(169), None, "BC1_UNORM is refused, not sized");
        assert_eq!(element_bytes(0), None, "INVALID is refused");
    }

    /// **The one measured byte, and the second point the software models agree on.**
    ///
    /// `(15,15)` → byte 4348 is the single texel read back on hardware (the point draw
    /// `166-agc/primitive-draw`, sweep `20260916-223136`). `(32,21)` → byte 2640 is **not** a second
    /// readback: it is what obSCEne's own tiler computes and this closed form, derived to fit the
    /// measured byte, independently reproduces - software-model agreement (`166-agc/tiling-swizzle`),
    /// not hardware. The assertions still pin both, because a swizzle wrong at either would break the
    /// detile; they just do not claim two measurements. Also made to fail against a linear layout
    /// (byte 4348 sits at `(63,16)` there).
    #[test]
    fn the_measured_byte_and_the_model_agreed_second_point() {
        assert_eq!(offset(15, 15), 4348, "the one measured byte");
        assert_eq!(offset(15, 15), 0x10fc);
        assert_eq!(
            offset(32, 21),
            2640,
            "the point obSCEne's tiler computes, reproduced"
        );
        assert_eq!(offset(32, 21), 0xa50);
        assert_ne!(offset(32, 21), 4348, "and not 4348");
    }

    /// **The origin is byte 0 and the element's own two low bits are never swizzled.**
    ///
    /// A decode folding a coordinate into bit 0 or 1 would read across the four-byte element.
    #[test]
    fn the_origin_is_zero_and_offsets_are_element_aligned() {
        assert_eq!(offset(0, 0), 0);
        for x in 0..16 {
            for y in 0..16 {
                assert_eq!(offset(x, y) & 0x3, 0, "texel ({x},{y}) is element-aligned");
            }
        }
    }

    /// **The swizzle is a bijection over a 128x128 block: no two texels share a byte.**
    ///
    /// A tiling mapping two texels to one address would lose data on detile. Enumerating the block
    /// confirms one-to-one - the property that makes `(15,15)`'s address unique.
    #[test]
    fn the_swizzle_is_a_bijection_over_the_block() {
        let mut seen = std::collections::HashSet::new();
        for y in 0..128 {
            for x in 0..128 {
                assert!(seen.insert(offset(x, y)), "texel ({x},{y}) collides");
            }
        }
        assert_eq!(seen.len(), 128 * 128);
    }

    /// **Detiling inverts the swizzle: a surface whose every texel carries its own `(x,y)` comes back
    /// row-major.**
    ///
    /// A round-trip against a reference pattern: fill each texel with `(y<<16)|x`, place it at its
    /// swizzled offset, and detiling must return the coordinates to their linear places. If the
    /// swizzle and its inverse disagreed anywhere, some texel would land at the wrong linear index.
    #[test]
    fn detile_returns_each_texel_to_its_linear_place() {
        let (width, height) = (16u32, 16u32);
        // Lay a tiled buffer out the way the hardware would: each texel's encoded (x,y) at its
        // swizzled offset. `+1` because the highest offset is inclusive.
        let mut needed = 0usize;
        for y in 0..height {
            for x in 0..width {
                needed = needed.max(offset(x, y) as usize / 4 + 1);
            }
        }
        let mut tiled = vec![0u32; needed];
        for y in 0..height {
            for x in 0..width {
                tiled[offset(x, y) as usize / 4] = (y << 16) | x;
            }
        }

        let linear = detile_64kb_rx_bpp4(&tiled, width, height).expect("fits in one block");
        for y in 0..height {
            for x in 0..width {
                assert_eq!(
                    linear[(y * width + x) as usize],
                    (y << 16) | x,
                    "texel ({x},{y}) landed wrong"
                );
            }
        }
    }

    /// **The measured pixel detiles to its linear place and nothing else moves.**
    ///
    /// A 64x64 surface (the `-a1f7` extent) with a single marker at the measured word 1087 must come
    /// back with that marker at linear index `15*64+15` and every other pixel clear - the detile form
    /// of the one hardware fact.
    #[test]
    fn the_measured_pixel_detiles_to_its_linear_index() {
        const RED: u32 = 0xff00_00ff;
        let (width, height) = (64u32, 64u32);
        let mut tiled = vec![0u32; 4096];
        tiled[4348 / 4] = RED; // the measured tiled byte, as a word index

        let linear = detile_64kb_rx_bpp4(&tiled, width, height).expect("64x64 fits in one block");
        assert_eq!(linear[(15 * width + 15) as usize], RED, "texel (15,15)");
        assert_eq!(
            linear.iter().filter(|&&p| p == RED).count(),
            1,
            "only the measured texel is red"
        );
    }

    /// **A surface past one block is refused, not silently mis-tiled.**
    #[test]
    fn a_surface_beyond_one_block_is_refused() {
        let tiled = vec![0u32; 16384];
        assert_eq!(
            detile_64kb_rx_bpp4(&tiled, 129, 64),
            Err(DetileError::SurfaceExceedsBlock {
                width: 129,
                height: 64
            })
        );
    }

    /// **Tiled data too short to cover the swizzle is refused rather than read out of bounds.**
    ///
    /// A 64x64 surface reaches word 1087 at least; one word shorter than it needs must refuse.
    #[test]
    fn tiled_data_too_short_is_refused() {
        let tiled = vec![0u32; 8];
        match detile_64kb_rx_bpp4(&tiled, 64, 64) {
            Err(DetileError::TiledDataTooShort { got_words, .. }) => assert_eq!(got_words, 8),
            other => panic!("expected a too-short refusal, got {other:?}"),
        }
    }
}
