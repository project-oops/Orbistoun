//! Every image format a guest's image descriptor can name, and what the host samples it as (D923).
//!
//! An image descriptor's `FORMAT` (`SQ_IMG_RSRC_WORD1` bits 20-28) is one code of Mesa's
//! `GFX10_FORMAT` enum, which carries both the bit layout and the number format
//! (`src/amd/registers/gfx10-rsrc.json:5-164`). Every code the enum names is in [`GFX10_FORMATS`]
//! with the line it is on, and a test checks the table against Mesa's file where one is checked out
//! beside this repository.
//!
//! A code's layout is read off its name as Mesa's own generator reads it
//! (`src/amd/common/gfx10_format_table.py:103-121`, `Gfx10Format`): the leading numbers are the
//! channels' widths from the most significant end, so reversed they run from the least
//! significant, the first channel - `X` - lowest. `10_11_11_FLOAT` is `X` and `Y` eleven bits and
//! `Z` ten from the bottom, which is Mesa's `PIPE_FORMAT_R11G11B10_FLOAT`
//! (`gfx10_format_table.py:37`).
//!
//! What each code becomes on the host is a [`Sampled`] answer: a host format that holds the same
//! bits, which channel of it each of the guest's `X` to `W` is ([`HostChannel`]), and the format a
//! CPU decode ([`decode`]) produces where the host cannot sample that one. A code nothing here reads
//! exactly is a [`Refusal`] that names why. Host formats are named as the Vulkan specification
//! names them; mapping them to an API is the backend's.

use crate::bc;

/// Every `GFX10_FORMAT` code: its value, its name without the `GFX10_FORMAT_` prefix, and its line
/// in Mesa's `src/amd/registers/gfx10-rsrc.json`.
pub const GFX10_FORMATS: [(u32, &str, u32); 160] = [
    (0, "INVALID", 5),
    (1, "8_UNORM", 6),
    (2, "8_SNORM", 7),
    (3, "8_USCALED", 8),
    (4, "8_SSCALED", 9),
    (5, "8_UINT", 10),
    (6, "8_SINT", 11),
    (7, "16_UNORM", 12),
    (8, "16_SNORM", 13),
    (9, "16_USCALED", 14),
    (10, "16_SSCALED", 15),
    (11, "16_UINT", 16),
    (12, "16_SINT", 17),
    (13, "16_FLOAT", 18),
    (14, "8_8_UNORM", 19),
    (15, "8_8_SNORM", 20),
    (16, "8_8_USCALED", 21),
    (17, "8_8_SSCALED", 22),
    (18, "8_8_UINT", 23),
    (19, "8_8_SINT", 24),
    (20, "32_UINT", 25),
    (21, "32_SINT", 26),
    (22, "32_FLOAT", 27),
    (23, "16_16_UNORM", 28),
    (24, "16_16_SNORM", 29),
    (25, "16_16_USCALED", 30),
    (26, "16_16_SSCALED", 31),
    (27, "16_16_UINT", 32),
    (28, "16_16_SINT", 33),
    (29, "16_16_FLOAT", 34),
    (30, "10_11_11_UNORM", 35),
    (31, "10_11_11_SNORM", 36),
    (32, "10_11_11_USCALED", 37),
    (33, "10_11_11_SSCALED", 38),
    (34, "10_11_11_UINT", 39),
    (35, "10_11_11_SINT", 40),
    (36, "10_11_11_FLOAT", 41),
    (37, "11_11_10_UNORM", 42),
    (38, "11_11_10_SNORM", 43),
    (39, "11_11_10_USCALED", 44),
    (40, "11_11_10_SSCALED", 45),
    (41, "11_11_10_UINT", 46),
    (42, "11_11_10_SINT", 47),
    (43, "11_11_10_FLOAT", 48),
    (44, "10_10_10_2_UNORM", 49),
    (45, "10_10_10_2_SNORM", 50),
    (46, "10_10_10_2_USCALED", 51),
    (47, "10_10_10_2_SSCALED", 52),
    (48, "10_10_10_2_UINT", 53),
    (49, "10_10_10_2_SINT", 54),
    (50, "2_10_10_10_UNORM", 55),
    (51, "2_10_10_10_SNORM", 56),
    (52, "2_10_10_10_USCALED", 57),
    (53, "2_10_10_10_SSCALED", 58),
    (54, "2_10_10_10_UINT", 59),
    (55, "2_10_10_10_SINT", 60),
    (56, "8_8_8_8_UNORM", 61),
    (57, "8_8_8_8_SNORM", 62),
    (58, "8_8_8_8_USCALED", 63),
    (59, "8_8_8_8_SSCALED", 64),
    (60, "8_8_8_8_UINT", 65),
    (61, "8_8_8_8_SINT", 66),
    (62, "32_32_UINT", 67),
    (63, "32_32_SINT", 68),
    (64, "32_32_FLOAT", 69),
    (65, "16_16_16_16_UNORM", 70),
    (66, "16_16_16_16_SNORM", 71),
    (67, "16_16_16_16_USCALED", 72),
    (68, "16_16_16_16_SSCALED", 73),
    (69, "16_16_16_16_UINT", 74),
    (70, "16_16_16_16_SINT", 75),
    (71, "16_16_16_16_FLOAT", 76),
    (72, "32_32_32_UINT", 77),
    (73, "32_32_32_SINT", 78),
    (74, "32_32_32_FLOAT", 79),
    (75, "32_32_32_32_UINT", 80),
    (76, "32_32_32_32_SINT", 81),
    (77, "32_32_32_32_FLOAT", 82),
    (128, "8_SRGB", 83),
    (129, "8_8_SRGB", 84),
    (130, "8_8_8_8_SRGB", 85),
    (131, "6E4_FLOAT", 86),
    (132, "5_9_9_9_FLOAT", 87),
    (133, "5_6_5_UNORM", 88),
    (134, "1_5_5_5_UNORM", 89),
    (135, "5_5_5_1_UNORM", 90),
    (136, "4_4_4_4_UNORM", 91),
    (137, "4_4_UNORM", 92),
    (138, "1_UNORM", 93),
    (139, "1_REVERSED_UNORM", 94),
    (140, "32_FLOAT_CLAMP", 95),
    (141, "8_24_UNORM", 96),
    (142, "8_24_UINT", 97),
    (143, "24_8_UNORM", 98),
    (144, "24_8_UINT", 99),
    (145, "X24_8_32_UINT", 100),
    (146, "X24_8_32_FLOAT", 101),
    (147, "GB_GR_UNORM", 102),
    (148, "GB_GR_SNORM", 103),
    (149, "GB_GR_UINT", 104),
    (150, "GB_GR_SRGB", 105),
    (151, "BG_RG_UNORM", 106),
    (152, "BG_RG_SNORM", 107),
    (153, "BG_RG_UINT", 108),
    (154, "BG_RG_SRGB", 109),
    (156, "FMASK8_S2_F1", 110),
    (157, "FMASK8_S4_F1", 111),
    (158, "FMASK8_S8_F1", 112),
    (159, "FMASK8_S2_F2", 113),
    (160, "FMASK8_S4_F2", 114),
    (161, "FMASK8_S4_F4", 115),
    (162, "FMASK16_S16_F1", 116),
    (163, "FMASK16_S8_F2", 117),
    (164, "FMASK32_S16_F2", 118),
    (165, "FMASK32_S8_F4", 119),
    (166, "FMASK32_S8_F8", 120),
    (167, "FMASK64_S16_F4", 121),
    (168, "FMASK64_S16_F8", 122),
    (169, "BC1_UNORM", 123),
    (170, "BC1_SRGB", 124),
    (171, "BC2_UNORM", 125),
    (172, "BC2_SRGB", 126),
    (173, "BC3_UNORM", 127),
    (174, "BC3_SRGB", 128),
    (175, "BC4_UNORM", 129),
    (176, "BC4_SNORM", 130),
    (177, "BC5_UNORM", 131),
    (178, "BC5_SNORM", 132),
    (179, "BC6_UFLOAT", 133),
    (180, "BC6_SFLOAT", 134),
    (181, "BC7_UNORM", 135),
    (182, "BC7_SRGB", 136),
    (265, "MM_8_UNORM", 137),
    (266, "MM_8_UINT", 138),
    (267, "MM_8_8_UNORM", 139),
    (268, "MM_8_8_UINT", 140),
    (269, "MM_8_8_8_8_UNORM", 141),
    (270, "MM_8_8_8_8_UINT", 142),
    (271, "MM_VYUY8_UNORM", 143),
    (272, "MM_VYUY8_UINT", 144),
    (273, "MM_10_11_11_UNORM", 145),
    (274, "MM_10_11_11_UINT", 146),
    (275, "MM_2_10_10_10_UNORM", 147),
    (276, "MM_2_10_10_10_UINT", 148),
    (277, "MM_16_16_16_16_UNORM", 149),
    (278, "MM_16_16_16_16_UINT", 150),
    (279, "MM_10_IN_16_UNORM", 151),
    (280, "MM_10_IN_16_UINT", 152),
    (281, "MM_10_IN_16_16_UNORM", 153),
    (282, "MM_10_IN_16_16_UINT", 154),
    (283, "MM_10_IN_16_16_16_16_UNORM", 155),
    (284, "MM_10_IN_16_16_16_16_UINT", 156),
    (285, "7E3_FLOAT", 157),
    (286, "YCBCR_UNORM", 158),
    (287, "YCBCR_SNORM", 159),
    (288, "YCBCR_USCALED", 160),
    (289, "YCBCR_SSCALED", 161),
    (290, "YCBCR_UINT", 162),
    (291, "YCBCR_SINT", 163),
    (292, "YCBCR_SRGB", 164),
];

/// A code's name in Mesa's enum, without its prefix; `None` for a code the enum does not name.
#[must_use]
pub fn name(code: u32) -> Option<&'static str> {
    GFX10_FORMATS
        .iter()
        .find(|(value, _, _)| *value == code)
        .map(|(_, name, _)| *name)
}

/// How a channel's bits are numbers (`gfx10_format_table.py:141-190`, the number formats a code's
/// name ends in).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Numeric {
    /// Unsigned, normalised to 0..1.
    Unorm,
    /// Two's complement, normalised to -1..1.
    Snorm,
    /// Unsigned integer.
    Uint,
    /// Two's-complement integer.
    Sint,
    /// Floating point: half and single floats, and the packed unsigned floats.
    Float,
    /// `Unorm` with the colour channels sRGB-encoded.
    Srgb,
}

impl Numeric {
    /// What a shader reads a channel of this as.
    #[must_use]
    pub const fn sampled_type(self) -> SampledType {
        match self {
            Self::Uint => SampledType::Uint,
            Self::Sint => SampledType::Sint,
            Self::Unorm | Self::Snorm | Self::Float | Self::Srgb => SampledType::Float,
        }
    }
}

/// What a shader's sample returns: floats, or unsigned or signed integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampledType {
    /// Floats, as normalised and floating-point formats read.
    Float,
    /// Unsigned integers.
    Uint,
    /// Signed integers.
    Sint,
}

/// An uncompressed layout: each channel's width from the least significant bit, `X` first, and
/// how its bits are numbers. A texel is the widths' sum, a whole number of bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Plain {
    /// The channels' widths, `X` first; zero past the last channel.
    pub widths: [u8; 4],
    /// How many channels a texel holds.
    pub channels: u8,
    /// How their bits are numbers.
    pub numeric: Numeric,
}

impl Plain {
    /// Bytes one texel takes.
    #[must_use]
    pub fn bytes(self) -> u32 {
        self.widths.iter().map(|&w| u32::from(w)).sum::<u32>() / 8
    }
}

/// A block-compressed format: four-by-four texels to a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Block {
    /// `BC1`, colour and a one-bit alpha.
    Bc1,
    /// `BC2`, colour and explicit four-bit alpha.
    Bc2,
    /// `BC3`, colour and interpolated alpha.
    Bc3,
    /// `BC4`, one channel.
    Bc4,
    /// `BC5`, two channels.
    Bc5,
    /// `BC6H`, three half-float channels.
    Bc6h,
    /// `BC7`.
    Bc7,
}

impl Block {
    /// Bytes one block takes (Data Format 21-23).
    #[must_use]
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Bc1 | Self::Bc4 => 8,
            Self::Bc2 | Self::Bc3 | Self::Bc5 | Self::Bc6h | Self::Bc7 => 16,
        }
    }
}

/// How a code's texels lie in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layout {
    /// One texel at a time, its channels packed from the least significant bit.
    Plain(Plain),
    /// `5_9_9_9_FLOAT`: three nine-bit mantissas from the low end and a shared five-bit exponent,
    /// Mesa's `PIPE_FORMAT_R9G9B9E5_FLOAT` (`gfx10_format_table.py:36`).
    SharedExponent,
    /// Four-by-four blocks, unsigned, signed (`SNORM`, `SFLOAT`) or sRGB-encoded.
    Blocks {
        /// Which algorithm.
        block: Block,
        /// Whether its values are signed.
        signed: bool,
        /// Whether its colour is sRGB-encoded.
        srgb: bool,
    },
}

impl Layout {
    /// The bytes of one element as tiling counts it: a texel, or a block.
    #[must_use]
    pub fn element_bytes(self) -> u32 {
        match self {
            Self::Plain(plain) => plain.bytes(),
            Self::SharedExponent => 4,
            Self::Blocks { block, .. } => block.bytes(),
        }
    }

    /// Texels to an element along each side: one, or a block's four.
    #[must_use]
    pub const fn element_extent(self) -> u32 {
        match self {
            Self::Blocks { .. } => 4,
            Self::Plain(_) | Self::SharedExponent => 1,
        }
    }

    /// What a shader reads it as.
    #[must_use]
    pub const fn sampled_type(self) -> SampledType {
        match self {
            Self::Plain(plain) => plain.numeric.sampled_type(),
            Self::SharedExponent | Self::Blocks { .. } => SampledType::Float,
        }
    }
}

/// Why a code is not sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The enum names no format at this code.
    Unnamed(u32),
    /// `INVALID`, code 0.
    Invalid,
    /// A format Mesa reads from buffers alone and never images: the scaled integers
    /// (`gfx10_format_table.py:149-152,163-166`) and the twelve-byte `32_32_32` formats, which
    /// the enum itself flags `buffers_only` (`gfx10-rsrc.json:77-79`).
    BuffersOnly(&'static str),
    /// A depth and stencil layout, read where depth is (`depth.rs`), not as a colour texture.
    DepthStencil(&'static str),
    /// A multisampled image's sample map.
    Fmask(&'static str),
    /// A format whose texels pair across two pixels or planes - `GB_GR`, `BG_RG`, `MM_*`,
    /// `YCBCR_*` - which a colour sample does not read alone.
    Video(&'static str),
    /// A format whose meaning no published source here gives: the small floats `6E4` and `7E3`,
    /// `32_FLOAT_CLAMP`, and the one-bit formats.
    Unpublished(&'static str),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unnamed(code) => write!(
                f,
                "image format {code}, which GFX10_FORMAT names no format at"
            ),
            Self::Invalid => write!(f, "image format 0, INVALID"),
            Self::BuffersOnly(name) => {
                write!(
                    f,
                    "image format {name}, which Mesa reads from buffers alone"
                )
            }
            Self::DepthStencil(name) => {
                write!(
                    f,
                    "image format {name}, a depth and stencil layout, not a colour texture"
                )
            }
            Self::Fmask(name) => {
                write!(f, "image format {name}, a multisampled image's sample map")
            }
            Self::Video(name) => {
                write!(
                    f,
                    "image format {name}, whose texels pair across pixels or planes"
                )
            }
            Self::Unpublished(name) => {
                write!(
                    f,
                    "image format {name}, whose texel meaning no published source gives"
                )
            }
        }
    }
}

/// A code's layout, read off its name (module docs); a [`Refusal`] for a code read no other way.
pub fn layout(code: u32) -> Result<Layout, Refusal> {
    let name = name(code).ok_or(Refusal::Unnamed(code))?;
    if code == 0 {
        return Err(Refusal::Invalid);
    }
    if let Some(found) = block_layout(name) {
        return Ok(found);
    }
    if name == "5_9_9_9_FLOAT" {
        return Ok(Layout::SharedExponent);
    }
    if name.starts_with("FMASK") {
        return Err(Refusal::Fmask(name));
    }
    if name.starts_with("MM_")
        || name.starts_with("YCBCR")
        || name.starts_with("GB_GR")
        || name.starts_with("BG_RG")
    {
        return Err(Refusal::Video(name));
    }
    if name.contains("24") {
        return Err(Refusal::DepthStencil(name));
    }
    if matches!(
        name,
        "6E4_FLOAT" | "7E3_FLOAT" | "32_FLOAT_CLAMP" | "1_UNORM" | "1_REVERSED_UNORM"
    ) {
        return Err(Refusal::Unpublished(name));
    }
    if name.starts_with("32_32_32_") && !name.starts_with("32_32_32_32") {
        return Err(Refusal::BuffersOnly(name));
    }
    let mut parts: Vec<&str> = name.split('_').collect();
    let numeric = match parts.pop() {
        Some("UNORM") => Numeric::Unorm,
        Some("SNORM") => Numeric::Snorm,
        Some("UINT") => Numeric::Uint,
        Some("SINT") => Numeric::Sint,
        Some("FLOAT") => Numeric::Float,
        Some("SRGB") => Numeric::Srgb,
        Some("USCALED" | "SSCALED") => return Err(Refusal::BuffersOnly(name)),
        _ => return Err(Refusal::Unpublished(name)),
    };
    // Most significant first in the name, so reversed: `X` lowest.
    let mut widths = [0_u8; 4];
    for (slot, part) in widths.iter_mut().zip(parts.iter().rev()) {
        *slot = part.parse().map_err(|_| Refusal::Unpublished(name))?;
    }
    Ok(Layout::Plain(Plain {
        widths,
        channels: parts.len() as u8,
        numeric,
    }))
}

/// A block-compressed code's layout, by its name.
fn block_layout(name: &str) -> Option<Layout> {
    let (block, rest) = name.split_once('_')?;
    let block = match block {
        "BC1" => Block::Bc1,
        "BC2" => Block::Bc2,
        "BC3" => Block::Bc3,
        "BC4" => Block::Bc4,
        "BC5" => Block::Bc5,
        "BC6" => Block::Bc6h,
        "BC7" => Block::Bc7,
        _ => return None,
    };
    Some(Layout::Blocks {
        block,
        signed: matches!(rest, "SNORM" | "SFLOAT"),
        srgb: rest == "SRGB",
    })
}

/// A host format that holds a guest layout's bits as they are, as the Vulkan specification names
/// it (section 49, "Formats").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs, reason = "each variant is the Vulkan format of its name")]
pub enum HostFormat {
    R8Unorm,
    R8Snorm,
    R8Uint,
    R8Sint,
    R8Srgb,
    R8G8Unorm,
    R8G8Snorm,
    R8G8Uint,
    R8G8Sint,
    R8G8Srgb,
    R8G8B8A8Unorm,
    R8G8B8A8Snorm,
    R8G8B8A8Uint,
    R8G8B8A8Sint,
    R8G8B8A8Srgb,
    R16Unorm,
    R16Snorm,
    R16Uint,
    R16Sint,
    R16Sfloat,
    R16G16Unorm,
    R16G16Snorm,
    R16G16Uint,
    R16G16Sint,
    R16G16Sfloat,
    R16G16B16A16Unorm,
    R16G16B16A16Snorm,
    R16G16B16A16Uint,
    R16G16B16A16Sint,
    R16G16B16A16Sfloat,
    R32Uint,
    R32Sint,
    R32Sfloat,
    R32G32Uint,
    R32G32Sint,
    R32G32Sfloat,
    R32G32B32A32Uint,
    R32G32B32A32Sint,
    R32G32B32A32Sfloat,
    A2B10G10R10UnormPack32,
    A2B10G10R10SnormPack32,
    A2B10G10R10UintPack32,
    A2B10G10R10SintPack32,
    B10G11R11UfloatPack32,
    E5B9G9R9UfloatPack32,
    R5G6B5UnormPack16,
    A1R5G5B5UnormPack16,
    R5G5B5A1UnormPack16,
    B4G4R4A4UnormPack16,
    R4G4UnormPack8,
    Bc1RgbaUnormBlock,
    Bc1RgbaSrgbBlock,
    Bc2UnormBlock,
    Bc2SrgbBlock,
    Bc3UnormBlock,
    Bc3SrgbBlock,
    Bc4UnormBlock,
    Bc4SnormBlock,
    Bc5UnormBlock,
    Bc5SnormBlock,
    Bc6hUfloatBlock,
    Bc6hSfloatBlock,
    Bc7UnormBlock,
    Bc7SrgbBlock,
}

/// Which channel of a host texel a guest channel is, or that the guest's format has no such
/// channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostChannel {
    /// The host's red.
    R,
    /// The host's green.
    G,
    /// The host's blue.
    B,
    /// The host's alpha.
    A,
    /// The guest's format holds no such channel. A host reads such a channel of a texel as zero
    /// for colour and one for alpha (Vulkan 16.3.3, "Conversion to RGBA"); whether the guest's
    /// sampler reads it so is not measured, so a select naming one is the caller's to refuse.
    Missing,
}

/// The format a CPU decode produces ([`decode`]): what a guest layout is widened to when the host
/// cannot sample it as it is. Every one is a format the Vulkan specification requires every
/// implementation to sample (section 49.3, "Required Format Support").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Widened {
    /// One word a texel, four eight-bit channels, red lowest: `R8G8B8A8_UNORM`.
    Rgba8Unorm,
    /// [`Self::Rgba8Unorm`] with the colour sRGB-encoded: `R8G8B8A8_SRGB`.
    Rgba8Srgb,
    /// Two words a texel, four half floats: `R16G16B16A16_SFLOAT`.
    Rgba16Float,
    /// Four words a texel, four floats: `R32G32B32A32_SFLOAT`.
    Rgba32Float,
    /// Four words a texel, four unsigned integers: `R32G32B32A32_UINT`.
    Rgba32Uint,
    /// Four words a texel, four signed integers: `R32G32B32A32_SINT`.
    Rgba32Sint,
}

impl Widened {
    /// Words one texel takes.
    #[must_use]
    pub const fn words(self) -> usize {
        match self {
            Self::Rgba8Unorm | Self::Rgba8Srgb => 1,
            Self::Rgba16Float => 2,
            Self::Rgba32Float | Self::Rgba32Uint | Self::Rgba32Sint => 4,
        }
    }
}

/// What a guest format is sampled as on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sampled {
    /// The format's layout.
    pub layout: Layout,
    /// A host format holding its bits as they are, where one exists.
    pub native: Option<HostFormat>,
    /// Where each of the guest's `X`, `Y`, `Z` and `W` is in a `native` texel.
    pub channels: [HostChannel; 4],
    /// What a CPU decode widens it to, its `X` to `W` the host's red to alpha.
    pub widened: Widened,
}

impl Sampled {
    /// The host channel each of a sample's four results reads under the guest's destination
    /// selects ([`crate::pipeline`]'s `DST_SEL`: 0 and 1 the constants, 4 to 7 the texel's `X` to
    /// `W`, `gfx6.json` `SQ_SEL_XYZW01`), on the `native` image when `native` and on the widened
    /// one otherwise. `None` where a select is reserved (2, 3) or names a channel the format lacks
    /// ([`HostChannel::Missing`]), and, for an sRGB format, where it moves a channel to another
    /// place: whether the console decodes sRGB before or after its selects is not measured
    /// (`REQ-20261021T0100Z-tx01` section 4), and the host decodes before.
    #[must_use]
    pub fn view(&self, selects: [u32; 4], native: bool) -> Option<[Swizzle; 4]> {
        let widened = [
            HostChannel::R,
            HostChannel::G,
            HostChannel::B,
            HostChannel::A,
        ];
        let srgb = matches!(
            self.layout,
            Layout::Plain(Plain {
                numeric: Numeric::Srgb,
                ..
            }) | Layout::Blocks { srgb: true, .. }
        );
        let channels = self.channels;
        let mut out = [Swizzle::Zero; 4];
        for (place, (slot, select)) in out.iter_mut().zip(selects).enumerate() {
            *slot = match select {
                0 => Swizzle::Zero,
                1 => Swizzle::One,
                4..=7 => {
                    let guest = (select - 4) as usize;
                    if channels[guest] == HostChannel::Missing || (srgb && guest != place) {
                        return None;
                    }
                    Swizzle::Channel(if native {
                        channels[guest]
                    } else {
                        widened[guest]
                    })
                }
                _ => return None,
            };
        }
        Some(out)
    }
}

/// What one channel of a sample returns: a host channel, or a constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Swizzle {
    /// The host texel's channel.
    Channel(HostChannel),
    /// Zero.
    Zero,
    /// One.
    One,
}

/// What the host samples `code` as, or why it does not.
pub fn sampled(code: u32) -> Result<Sampled, Refusal> {
    let layout = layout(code)?;
    let (native, channels) = native(layout);
    Ok(Sampled {
        layout,
        native,
        channels,
        widened: widened(layout),
    })
}

/// The guest's channels as a host format with the same channels in the same order holds them.
fn in_order(count: u8) -> [HostChannel; 4] {
    let all = [
        HostChannel::R,
        HostChannel::G,
        HostChannel::B,
        HostChannel::A,
    ];
    core::array::from_fn(|i| {
        if i < usize::from(count) {
            all[i]
        } else {
            HostChannel::Missing
        }
    })
}

/// A host format holding `layout`'s bits as they are, and where the guest's channels are in it.
#[allow(
    clippy::too_many_lines,
    reason = "one table of formats, each line its own case"
)]
fn native(layout: Layout) -> (Option<HostFormat>, [HostChannel; 4]) {
    use HostChannel::{A, B, G, Missing, R};
    use HostFormat as H;
    use Numeric::{Float, Sint, Snorm, Srgb, Uint, Unorm};
    let plain = match layout {
        Layout::SharedExponent => {
            return (Some(H::E5B9G9R9UfloatPack32), [R, G, B, Missing]);
        }
        Layout::Blocks {
            block,
            signed,
            srgb,
        } => {
            let (format, count) = match (block, signed, srgb) {
                (Block::Bc1, _, false) => (H::Bc1RgbaUnormBlock, 4),
                (Block::Bc1, _, true) => (H::Bc1RgbaSrgbBlock, 4),
                (Block::Bc2, _, false) => (H::Bc2UnormBlock, 4),
                (Block::Bc2, _, true) => (H::Bc2SrgbBlock, 4),
                (Block::Bc3, _, false) => (H::Bc3UnormBlock, 4),
                (Block::Bc3, _, true) => (H::Bc3SrgbBlock, 4),
                (Block::Bc4, false, _) => (H::Bc4UnormBlock, 1),
                (Block::Bc4, true, _) => (H::Bc4SnormBlock, 1),
                (Block::Bc5, false, _) => (H::Bc5UnormBlock, 2),
                (Block::Bc5, true, _) => (H::Bc5SnormBlock, 2),
                (Block::Bc6h, false, _) => (H::Bc6hUfloatBlock, 3),
                (Block::Bc6h, true, _) => (H::Bc6hSfloatBlock, 3),
                (Block::Bc7, _, false) => (H::Bc7UnormBlock, 4),
                (Block::Bc7, _, true) => (H::Bc7SrgbBlock, 4),
            };
            return (Some(format), in_order(count));
        }
        Layout::Plain(plain) => plain,
    };
    let order = in_order(plain.channels);
    let format = match (plain.widths, plain.numeric) {
        ([8, 0, 0, 0], Unorm) => H::R8Unorm,
        ([8, 0, 0, 0], Snorm) => H::R8Snorm,
        ([8, 0, 0, 0], Uint) => H::R8Uint,
        ([8, 0, 0, 0], Sint) => H::R8Sint,
        ([8, 0, 0, 0], Srgb) => H::R8Srgb,
        ([8, 8, 0, 0], Unorm) => H::R8G8Unorm,
        ([8, 8, 0, 0], Snorm) => H::R8G8Snorm,
        ([8, 8, 0, 0], Uint) => H::R8G8Uint,
        ([8, 8, 0, 0], Sint) => H::R8G8Sint,
        ([8, 8, 0, 0], Srgb) => H::R8G8Srgb,
        ([8, 8, 8, 8], Unorm) => H::R8G8B8A8Unorm,
        ([8, 8, 8, 8], Snorm) => H::R8G8B8A8Snorm,
        ([8, 8, 8, 8], Uint) => H::R8G8B8A8Uint,
        ([8, 8, 8, 8], Sint) => H::R8G8B8A8Sint,
        ([8, 8, 8, 8], Srgb) => H::R8G8B8A8Srgb,
        ([16, 0, 0, 0], Unorm) => H::R16Unorm,
        ([16, 0, 0, 0], Snorm) => H::R16Snorm,
        ([16, 0, 0, 0], Uint) => H::R16Uint,
        ([16, 0, 0, 0], Sint) => H::R16Sint,
        ([16, 0, 0, 0], Float) => H::R16Sfloat,
        ([16, 16, 0, 0], Unorm) => H::R16G16Unorm,
        ([16, 16, 0, 0], Snorm) => H::R16G16Snorm,
        ([16, 16, 0, 0], Uint) => H::R16G16Uint,
        ([16, 16, 0, 0], Sint) => H::R16G16Sint,
        ([16, 16, 0, 0], Float) => H::R16G16Sfloat,
        ([16, 16, 16, 16], Unorm) => H::R16G16B16A16Unorm,
        ([16, 16, 16, 16], Snorm) => H::R16G16B16A16Snorm,
        ([16, 16, 16, 16], Uint) => H::R16G16B16A16Uint,
        ([16, 16, 16, 16], Sint) => H::R16G16B16A16Sint,
        ([16, 16, 16, 16], Float) => H::R16G16B16A16Sfloat,
        ([32, 0, 0, 0], Uint) => H::R32Uint,
        ([32, 0, 0, 0], Sint) => H::R32Sint,
        ([32, 0, 0, 0], Float) => H::R32Sfloat,
        ([32, 32, 0, 0], Uint) => H::R32G32Uint,
        ([32, 32, 0, 0], Sint) => H::R32G32Sint,
        ([32, 32, 0, 0], Float) => H::R32G32Sfloat,
        ([32, 32, 32, 32], Uint) => H::R32G32B32A32Uint,
        ([32, 32, 32, 32], Sint) => H::R32G32B32A32Sint,
        ([32, 32, 32, 32], Float) => H::R32G32B32A32Sfloat,
        // Red the low ten bits, alpha the top two: the order Vulkan's `A2B10G10R10` packs, most
        // significant first.
        ([10, 10, 10, 2], Unorm) => H::A2B10G10R10UnormPack32,
        ([10, 10, 10, 2], Snorm) => H::A2B10G10R10SnormPack32,
        ([10, 10, 10, 2], Uint) => H::A2B10G10R10UintPack32,
        ([10, 10, 10, 2], Sint) => H::A2B10G10R10SintPack32,
        ([11, 11, 10, 0], Float) => H::B10G11R11UfloatPack32,
        // `X` in the low five bits, which `R5G6B5` holds blue in.
        ([5, 6, 5, 0], Unorm) => return (Some(H::R5G6B5UnormPack16), [B, G, R, Missing]),
        // `X` the low five bits, `W` the top one: `A1R5G5B5`'s blue, green, red and alpha.
        ([5, 5, 5, 1], Unorm) => return (Some(H::A1R5G5B5UnormPack16), [B, G, R, A]),
        // `X` the low bit: `R5G5B5A1`'s alpha, then its blue, green and red.
        ([1, 5, 5, 5], Unorm) => return (Some(H::R5G5B5A1UnormPack16), [A, B, G, R]),
        // `X` the low four bits, which `B4G4R4A4` holds alpha in, then red, green and blue.
        ([4, 4, 4, 4], Unorm) => return (Some(H::B4G4R4A4UnormPack16), [A, R, G, B]),
        // `X` the low four bits, `R4G4`'s green.
        ([4, 4, 0, 0], Unorm) => return (Some(H::R4G4UnormPack8), [G, R, Missing, Missing]),
        _ => return (None, order),
    };
    (Some(format), order)
}

/// What a CPU decode of `layout` produces.
fn widened(layout: Layout) -> Widened {
    match layout {
        Layout::Plain(plain) => match plain.numeric {
            Numeric::Uint => Widened::Rgba32Uint,
            Numeric::Sint => Widened::Rgba32Sint,
            Numeric::Srgb => Widened::Rgba8Srgb,
            Numeric::Unorm if plain.widths.iter().all(|&w| w <= 8) => Widened::Rgba8Unorm,
            Numeric::Unorm | Numeric::Snorm | Numeric::Float => Widened::Rgba32Float,
        },
        Layout::SharedExponent => Widened::Rgba32Float,
        Layout::Blocks {
            block,
            signed,
            srgb,
            ..
        } => match (block, signed, srgb) {
            (Block::Bc6h, ..) => Widened::Rgba16Float,
            (Block::Bc4 | Block::Bc5, true, _) => Widened::Rgba32Float,
            (_, _, true) => Widened::Rgba8Srgb,
            _ => Widened::Rgba8Unorm,
        },
    }
}

/// `bits` of an unsigned value normalised to 0..1.
fn unorm(value: u32, bits: u32) -> f32 {
    (f64::from(value) / f64::from(u32::MAX >> (32 - bits))) as f32
}

/// `bits` of a two's-complement value normalised to -1..1, the most negative read as -1 (Vulkan
/// 3.9.2, "Conversion from Normalized Fixed-Point to Floating-Point").
fn snorm(value: u32, bits: u32) -> f32 {
    let signed = f64::from(sign_extend(value, bits));
    (signed / f64::from(i32::MAX >> (32 - bits))).max(-1.0) as f32
}

const fn sign_extend(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

/// An unsigned float of a five-bit exponent and `mantissa` bits (Vulkan 3.6, "Unsigned 11-Bit
/// Floating-Point Numbers" and "Unsigned 10-Bit").
pub(crate) fn small_float(value: u32, mantissa: u32) -> f32 {
    let exponent = value >> mantissa;
    let bits = value & ((1 << mantissa) - 1);
    let fraction = f64::from(bits) / f64::from(1_u32 << mantissa);
    match exponent {
        0 => (fraction * 2_f64.powi(-14)) as f32,
        31 if bits == 0 => f32::INFINITY,
        31 => f32::NAN,
        _ => (2_f64.powi(exponent as i32 - 15) * (1.0 + fraction)) as f32,
    }
}

/// A half float's bits as a float (IEEE 754 binary16).
#[must_use]
pub fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    sign * small_float(u32::from(bits & 0x7FFF), 10)
}

/// One plain texel of `bits`, its `X` to `W` as floats or integers' bits by its numeric, missing
/// channels zero and alpha one.
fn plain_texel(plain: Plain, bits: u128) -> [u32; 4] {
    let mut out = match plain.numeric.sampled_type() {
        SampledType::Float => [0, 0, 0, 1.0_f32.to_bits()],
        SampledType::Uint | SampledType::Sint => [0, 0, 0, 1],
    };
    let mut at = 0;
    let channels = usize::from(plain.channels);
    for (slot, &width) in out.iter_mut().zip(&plain.widths).take(channels) {
        let width = u32::from(width);
        let raw = ((bits >> at) & ((1_u128 << width) - 1)) as u32;
        at += width;
        *slot = match (plain.numeric, width) {
            (Numeric::Uint, _) | (Numeric::Float, 32) => raw,
            (Numeric::Sint, _) => sign_extend(raw, width) as u32,
            (Numeric::Unorm | Numeric::Srgb, _) => unorm(raw, width).to_bits(),
            (Numeric::Snorm, _) => snorm(raw, width).to_bits(),
            (Numeric::Float, 16) => half_to_f32(raw as u16).to_bits(),
            (Numeric::Float, 11) => small_float(raw, 6).to_bits(),
            (Numeric::Float, _) => small_float(raw, 5).to_bits(),
        };
    }
    out
}

/// `5_9_9_9_FLOAT`'s texel as three floats and alpha one (Vulkan 3.6, "Shared Exponent").
fn shared_exponent(word: u32) -> [u32; 4] {
    let scale = 2_f64.powi((word >> 27) as i32 - 15 - 9);
    let channel = |at: u32| ((f64::from(word >> at & 0x1FF) * scale) as f32).to_bits();
    [channel(0), channel(9), channel(18), 1.0_f32.to_bits()]
}

/// A float as `UNORM` eight bits, rounded to nearest.
fn to_unorm8(value: f32) -> u32 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u32
}

/// `width` x `height` texels of `layout` from `bytes` - texels row-major, or blocks row-major for a
/// block-compressed layout - widened as [`Sampled::widened`] says, the words a host image of that
/// format is made from. `None` when `bytes` is too short.
pub fn decode(sampled: &Sampled, bytes: &[u8], (width, height): (u32, u32)) -> Option<Vec<u32>> {
    let (w, h) = (width as usize, height as usize);
    let widened = sampled.widened;
    let mut out = vec![0_u32; w * h * widened.words()];
    let mut store = |x: usize, y: usize, texel: [u32; 4]| {
        let at = (y * w + x) * widened.words();
        match widened {
            Widened::Rgba8Unorm | Widened::Rgba8Srgb => {
                out[at] = texel
                    .iter()
                    .enumerate()
                    .map(|(i, &c)| to_unorm8(f32::from_bits(c)) << (8 * i))
                    .sum();
            }
            Widened::Rgba16Float => {
                out[at] = texel[0] | texel[1] << 16;
                out[at + 1] = texel[2] | texel[3] << 16;
            }
            Widened::Rgba32Float | Widened::Rgba32Uint | Widened::Rgba32Sint => {
                out[at..at + 4].copy_from_slice(&texel);
            }
        }
    };
    match sampled.layout {
        Layout::Plain(_) | Layout::SharedExponent => {
            let size = sampled.layout.element_bytes() as usize;
            if bytes.len() < w * h * size {
                return None;
            }
            for y in 0..h {
                for x in 0..w {
                    let at = (y * w + x) * size;
                    let mut raw = [0_u8; 16];
                    raw[..size].copy_from_slice(&bytes[at..at + size]);
                    let bits = u128::from_le_bytes(raw);
                    let texel = match sampled.layout {
                        Layout::Plain(plain) => plain_texel(plain, bits),
                        _ => shared_exponent(bits as u32),
                    };
                    store(x, y, texel);
                }
            }
        }
        Layout::Blocks { block, signed, .. } => {
            let size = block.bytes() as usize;
            let across = w.div_ceil(4);
            if bytes.len() < across * h.div_ceil(4) * size {
                return None;
            }
            for (index, chunk) in bytes
                .chunks_exact(size)
                .enumerate()
                .take(across * h.div_ceil(4))
            {
                let texels = block_texels(block, signed, chunk);
                let (bx, by) = (index % across * 4, index / across * 4);
                for (i, texel) in texels.into_iter().enumerate() {
                    let (x, y) = (bx + i % 4, by + i / 4);
                    if x < w && y < h {
                        store(x, y, texel);
                    }
                }
            }
        }
    }
    Some(out)
}

/// One block's sixteen texels, as floats' bits, or for `BC6H` half floats' bits.
fn block_texels(block: Block, signed: bool, chunk: &[u8]) -> [[u32; 4]; 16] {
    let eight = || -> [u8; 8] { core::array::from_fn(|i| chunk[i]) };
    let sixteen = || -> [u8; 16] { core::array::from_fn(|i| chunk[i]) };
    let from_bytes =
        |texels: bc::Rgba8Block| texels.map(|t| t.map(|c| (f32::from(c) / 255.0).to_bits()));
    let from_floats = |texels: [[f32; 4]; 16]| texels.map(|t| t.map(f32::to_bits));
    match (block, signed) {
        (Block::Bc1, _) => from_bytes(bc::bc1(&eight())),
        (Block::Bc2, _) => from_bytes(bc::bc2(&sixteen())),
        (Block::Bc3, _) => from_bytes(bc::bc3(&sixteen())),
        (Block::Bc4, false) => from_bytes(bc::bc4_unorm(&eight())),
        (Block::Bc4, true) => from_floats(bc::bc4_snorm(&eight())),
        (Block::Bc5, false) => from_bytes(bc::bc5_unorm(&sixteen())),
        (Block::Bc5, true) => from_floats(bc::bc5_snorm(&sixteen())),
        (Block::Bc7, _) => from_bytes(bc::bc7(&sixteen())),
        // Half floats, alpha one (0x3C00).
        (Block::Bc6h, _) => bc::bc6h(&sixteen(), signed)
            .map(|[r, g, b]| [u32::from(r), u32::from(g), u32::from(b), 0x3C00]),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Block, GFX10_FORMATS, HostChannel, HostFormat, Layout, Numeric, Plain, Refusal, Swizzle,
        Widened, decode, layout, sampled,
    };

    /// Mesa's enum beside this repository, where it is checked out: `ORBISTOUN_MESA` or
    /// `../oops-mesa/mesa`.
    fn mesa_registers() -> Option<String> {
        let root = std::env::var("ORBISTOUN_MESA")
            .unwrap_or_else(|_| format!("{}/../../../oops-mesa/mesa", env!("CARGO_MANIFEST_DIR")));
        std::fs::read_to_string(format!("{root}/src/amd/registers/gfx10-rsrc.json")).ok()
    }

    /// Every code, name and line in [`GFX10_FORMATS`] is the one Mesa's file has, and the table
    /// has all of them: a code whose name was misread would be sampled as another format. A code
    /// the enum flags `buffers_only` is refused as an image.
    #[test]
    fn the_table_is_mesas_enum() {
        let Some(json) = mesa_registers() else {
            println!("SKIPPED - no Mesa checkout beside this repository; set ORBISTOUN_MESA");
            return;
        };
        let lines: Vec<&str> = json.lines().collect();
        let mut seen = 0;
        for (number, line) in lines.iter().enumerate() {
            let Some((flags, rest)) = line.split_once("\"name\": \"GFX10_FORMAT_") else {
                continue;
            };
            let (name, value) = rest.split_once("\", \"value\": ").expect("a value follows");
            let value: u32 = value
                .trim_end_matches(['}', ','])
                .parse()
                .expect("a number");
            let entry = GFX10_FORMATS
                .iter()
                .find(|(code, _, _)| *code == value)
                .unwrap_or_else(|| panic!("code {value} is missing"));
            assert_eq!(
                (entry.1, entry.2),
                (name.trim(), number as u32 + 1),
                "code {value}"
            );
            if flags.contains("buffers_only") {
                assert_eq!(
                    sampled(value),
                    Err(Refusal::BuffersOnly(entry.1)),
                    "code {value}"
                );
            }
            seen += 1;
        }
        assert_eq!(
            seen,
            GFX10_FORMATS.len(),
            "the table names codes Mesa's file does not"
        );
    }

    /// A name's widths run from the least significant bit, as Mesa reverses them: `10_11_11` is
    /// `X` and `Y` eleven bits and `Z` ten, the bits Vulkan's `B10G11R11` holds.
    #[test]
    fn a_name_lists_its_channels_from_the_top() {
        assert_eq!(
            layout(36),
            Ok(Layout::Plain(Plain {
                widths: [11, 11, 10, 0],
                channels: 3,
                numeric: Numeric::Float
            }))
        );
        let packed = sampled(36).expect("sampled");
        assert_eq!(packed.native, Some(HostFormat::B10G11R11UfloatPack32));
        // `5_6_5`: `X` is the low five bits, which `R5G6B5` calls blue.
        let rgb = sampled(133).expect("sampled");
        assert_eq!(rgb.native, Some(HostFormat::R5G6B5UnormPack16));
        assert_eq!(rgb.channels[0], HostChannel::B);
    }

    /// Every code is sampled or refused by name, and every block-compressed one - all fourteen of
    /// `BC1` to `BC7`'s variants - is sampled.
    #[test]
    fn every_code_is_sampled_or_refused_by_name() {
        let mut sampled_codes = 0;
        for &(code, name, _) in &GFX10_FORMATS {
            match sampled(code) {
                Ok(found) => {
                    sampled_codes += 1;
                    assert!(found.layout.element_bytes() > 0, "{name}");
                }
                Err(why) => assert!(why.to_string().contains(name) || code == 0, "{name}: {why}"),
            }
        }
        let blocks = (169..=182)
            .filter(|&code| matches!(layout(code), Ok(Layout::Blocks { .. })))
            .count();
        assert_eq!(blocks, 14);
        assert_eq!(sampled(155), Err(Refusal::Unnamed(155)));
        assert!(sampled_codes >= 77, "{sampled_codes} sampled");
    }

    /// A select of the constants or of a channel the format holds is a host swizzle; one of a
    /// channel it lacks, or a reserved code, is refused rather than read as the host would.
    #[test]
    fn selects_compose_with_the_formats_channels() {
        let rgb = sampled(133).expect("5_6_5");
        assert_eq!(
            rgb.view([4, 5, 6, 1], true),
            Some([
                Swizzle::Channel(HostChannel::B),
                Swizzle::Channel(HostChannel::G),
                Swizzle::Channel(HostChannel::R),
                Swizzle::One
            ])
        );
        assert_eq!(rgb.view([4, 5, 6, 7], true), None, "5_6_5 has no W");
        assert_eq!(
            rgb.view([6, 5, 4, 1], false).map(|v| v[0]),
            Some(Swizzle::Channel(HostChannel::B)),
            "widened, Z is the host's blue"
        );
        assert_eq!(rgb.view([2, 5, 6, 1], true), None, "a reserved select");
        // An sRGB format's channels are read in their places alone, alpha among them.
        let srgb = sampled(130).expect("8_8_8_8_SRGB");
        assert!(srgb.view([4, 5, 6, 7], true).is_some());
        assert!(
            srgb.view([4, 5, 6, 1], true).is_some(),
            "a constant in a place"
        );
        assert_eq!(srgb.view([7, 6, 5, 4], true), None, "channels moved");
    }

    /// A CPU decode widens each layout to its texels' values: `16_16_FLOAT` to floats, `8_8_SNORM`'s
    /// most negative to -1, `2_10_10_10_UINT` to integers, `5_9_9_9` to its shared-exponent floats.
    #[test]
    fn a_cpu_decode_widens_each_layout_to_its_values() {
        let half = sampled(29).expect("16_16_FLOAT");
        assert_eq!(half.widened, Widened::Rgba32Float);
        let words = decode(&half, &[0x00, 0x3C, 0x00, 0xC0], (1, 1)).expect("decoded");
        assert_eq!(
            words.iter().map(|&w| f32::from_bits(w)).collect::<Vec<_>>(),
            [1.0, -2.0, 0.0, 1.0]
        );
        let snorm = decode(&sampled(15).expect("8_8_SNORM"), &[0x80, 0x7F], (1, 1)).expect("ok");
        assert_eq!(
            (f32::from_bits(snorm[0]), f32::from_bits(snorm[1])),
            (-1.0, 1.0)
        );
        let int = sampled(54).expect("2_10_10_10_UINT");
        let word: u32 = 1 | 2 << 10 | 3 << 20 | 1 << 30;
        assert_eq!(
            decode(&int, &word.to_le_bytes(), (1, 1)),
            Some(vec![1, 2, 3, 1])
        );
        // Mantissas 256 in each channel and exponent 16: 256 x 2^(16 - 24) = 1.
        let shared = sampled(132).expect("5_9_9_9_FLOAT");
        let word: u32 = 256 | 256 << 9 | 256 << 18 | 16 << 27;
        let values = decode(&shared, &word.to_le_bytes(), (1, 1)).expect("decoded");
        assert_eq!(values[0], 1.0_f32.to_bits());
    }

    /// A block-compressed image decodes block by block into its texels' places, a partial last
    /// block cut at the image's edge.
    #[test]
    fn blocks_decode_into_their_texels_places() {
        let bc1 = sampled(169).expect("BC1_UNORM");
        assert_eq!(
            bc1.layout,
            Layout::Blocks {
                block: Block::Bc1,
                signed: false,
                srgb: false
            }
        );
        // Two blocks across: red then green, each of two equal endpoints.
        let block = |c: u16| {
            let [lo, hi] = c.to_le_bytes();
            [lo, hi, lo, hi, 0, 0, 0, 0]
        };
        let bytes = [block(0xF800), block(0x07E0)].concat();
        let words = decode(&bc1, &bytes, (6, 3)).expect("decoded");
        assert_eq!(words.len(), 18);
        assert_eq!(words[0], 0xFF00_00FF, "red");
        assert_eq!(words[5], 0xFF00_FF00, "green, the second block's");
        assert_eq!(words[17], 0xFF00_FF00);
    }
}
