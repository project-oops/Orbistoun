//! Block-compressed texels decoded on the CPU, for a host that cannot sample a guest's blocks
//! itself (D923).
//!
//! Every format here is one four-by-four block of texels, eight or sixteen bytes, its texels
//! row-major from the top left. The algorithms are the published ones: the Khronos Data Format
//! Specification 1.3 section 21 (S3TC: `BC1`-`BC3`), section 22 (RGTC: `BC4`, `BC5`) and section 23
//! (BPTC: `BC6H`, `BC7`), with Microsoft's "Texture Block Compression in Direct3D 11" for the
//! `BC6H` and `BC7` mode tables. A host that samples these formats natively decodes them itself;
//! this is the path for one that does not, and the device tests compare the two
//! (`crates/orbistoun-gpu-vulkan/tests/texture_formats.rs`).
//!
//! Interpolated values are rounded to nearest. Hardware decoders differ from one another by a unit
//! in the last place on those values, which the specifications allow (Data Format 21.1), so the
//! comparison with a native decode is within one unit.

/// Sixteen texels, row-major from the top left, four eight-bit channels each, red first.
pub type Rgba8Block = [[u8; 4]; 16];

/// Sixteen texels of red, green and blue half floats, as their bits.
pub type HalfBlock = [[u16; 3]; 16];

/// An eight-bit channel from `bits` of precision `width`, the top bits repeated into the low ones
/// (Data Format 21.1, the `RGB565` endpoints; 23.2, the `BC7` endpoints).
const fn widen(value: u32, width: u32) -> u8 {
    let shifted = value << (8 - width);
    (shifted | (shifted >> width)) as u8
}

/// `RGB565` as three eight-bit channels.
const fn rgb565(value: u16) -> [u8; 3] {
    let value = value as u32;
    [
        widen(value >> 11, 5),
        widen((value >> 5) & 0x3F, 6),
        widen(value & 0x1F, 5),
    ]
}

/// `(a * weight_a + b * weight_b) / divisor`, rounded to nearest.
const fn blend(a: u8, b: u8, weight_a: u32, weight_b: u32, divisor: u32) -> u8 {
    ((a as u32 * weight_a + b as u32 * weight_b + divisor / 2) / divisor) as u8
}

/// A `BC1` colour block's four colours, alpha included: the four-colour form when the first
/// endpoint is greater or `four_colour` forces it (`BC2` and `BC3` always decode so, Data Format
/// 21.2-21.3), else three colours and transparent black.
fn bc1_palette(block: &[u8], four_colour: bool) -> [[u8; 4]; 4] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let opaque = |rgb: [u8; 3]| [rgb[0], rgb[1], rgb[2], 255];
    let mix = |wa, wb, d| opaque([0, 1, 2].map(|i| blend(a[i], b[i], wa, wb, d)));
    if c0 > c1 || four_colour {
        [opaque(a), opaque(b), mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [opaque(a), opaque(b), mix(1, 1, 2), [0, 0, 0, 0]]
    }
}

/// The colour half of a `BC1`, `BC2` or `BC3` block: each texel's two-bit index into its palette.
fn colour_block(block: &[u8], four_colour: bool) -> Rgba8Block {
    let palette = bc1_palette(block, four_colour);
    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    core::array::from_fn(|texel| palette[(indices >> (2 * texel) & 3) as usize])
}

/// A `BC1` block (Data Format 21.1), with the one-bit alpha a three-colour block's fourth index
/// selects: `BC1_UNORM` and `BC1_SRGB` decode as Vulkan's `BC1_RGBA` forms do.
#[must_use]
pub fn bc1(block: &[u8; 8]) -> Rgba8Block {
    colour_block(block, false)
}

/// A `BC2` block (Data Format 21.2): sixteen four-bit alphas, then a `BC1` colour block always in
/// its four-colour form.
#[must_use]
pub fn bc2(block: &[u8; 16]) -> Rgba8Block {
    let alphas = u64::from_le_bytes(first_eight(block));
    let mut texels = colour_block(&block[8..], true);
    for (at, texel) in texels.iter_mut().enumerate() {
        texel[3] = widen((alphas >> (4 * at) & 0xF) as u32, 4);
    }
    texels
}

/// A `BC3` block (Data Format 21.3): a `BC4` block for alpha, then a `BC1` colour block in its
/// four-colour form.
#[must_use]
pub fn bc3(block: &[u8; 16]) -> Rgba8Block {
    let alpha = unsigned_channel(first_eight(block));
    let mut texels = colour_block(&block[8..], true);
    for (texel, value) in texels.iter_mut().zip(alpha) {
        texel[3] = value;
    }
    texels
}

fn first_eight(block: &[u8; 16]) -> [u8; 8] {
    core::array::from_fn(|i| block[i])
}

fn last_eight(block: &[u8; 16]) -> [u8; 8] {
    core::array::from_fn(|i| block[8 + i])
}

/// The three-bit indices of a one-channel block, texel `i`'s at bit `3 * i` of bytes 2 to 7.
fn channel_indices(block: [u8; 8]) -> u64 {
    u64::from_le_bytes(block) >> 16
}

/// An unsigned one-channel block (Data Format 22.1, `BC4_UNORM`): eight values between its two
/// endpoints when the first is greater, else six and the two extremes.
fn unsigned_channel(block: [u8; 8]) -> [u8; 16] {
    let (a, b) = (block[0], block[1]);
    let palette: [u8; 8] = if a > b {
        [
            a,
            b,
            blend(a, b, 6, 1, 7),
            blend(a, b, 5, 2, 7),
            blend(a, b, 4, 3, 7),
            blend(a, b, 3, 4, 7),
            blend(a, b, 2, 5, 7),
            blend(a, b, 1, 6, 7),
        ]
    } else {
        [
            a,
            b,
            blend(a, b, 4, 1, 5),
            blend(a, b, 3, 2, 5),
            blend(a, b, 2, 3, 5),
            blend(a, b, 1, 4, 5),
            0,
            255,
        ]
    };
    let indices = channel_indices(block);
    core::array::from_fn(|texel| palette[(indices >> (3 * texel) & 7) as usize])
}

/// A signed one-channel block (Data Format 22.2, `BC4_SNORM`) as values in -1 to 1: its endpoints
/// two's-complement bytes, -128 read as -127, compared signed.
fn signed_channel(block: [u8; 8]) -> [f32; 16] {
    let endpoint = |byte: u8| f32::from((byte as i8).max(-127)) / 127.0;
    let (a, b) = (endpoint(block[0]), endpoint(block[1]));
    let mix = |wa: f32, wb: f32, d: f32| (a * wa + b * wb) / d;
    let palette: [f32; 8] = if (block[0] as i8) > (block[1] as i8) {
        [
            a,
            b,
            mix(6.0, 1.0, 7.0),
            mix(5.0, 2.0, 7.0),
            mix(4.0, 3.0, 7.0),
            mix(3.0, 4.0, 7.0),
            mix(2.0, 5.0, 7.0),
            mix(1.0, 6.0, 7.0),
        ]
    } else {
        [
            a,
            b,
            mix(4.0, 1.0, 5.0),
            mix(3.0, 2.0, 5.0),
            mix(2.0, 3.0, 5.0),
            mix(1.0, 4.0, 5.0),
            -1.0,
            1.0,
        ]
    };
    let indices = channel_indices(block);
    core::array::from_fn(|texel| palette[(indices >> (3 * texel) & 7) as usize])
}

/// A `BC4_UNORM` block: red, with green and blue zero and alpha one, as a one-channel format reads.
#[must_use]
pub fn bc4_unorm(block: &[u8; 8]) -> Rgba8Block {
    unsigned_channel(*block).map(|red| [red, 0, 0, 255])
}

/// A `BC4_SNORM` block, red in -1 to 1, green and blue zero and alpha one.
#[must_use]
pub fn bc4_snorm(block: &[u8; 8]) -> [[f32; 4]; 16] {
    signed_channel(*block).map(|red| [red, 0.0, 0.0, 1.0])
}

/// A `BC5_UNORM` block (Data Format 22.3): a red block then a green one, blue zero and alpha one.
#[must_use]
pub fn bc5_unorm(block: &[u8; 16]) -> Rgba8Block {
    let (red, green) = (
        unsigned_channel(first_eight(block)),
        unsigned_channel(last_eight(block)),
    );
    core::array::from_fn(|texel| [red[texel], green[texel], 0, 255])
}

/// A `BC5_SNORM` block (Data Format 22.4): two signed channels, blue zero and alpha one.
#[must_use]
pub fn bc5_snorm(block: &[u8; 16]) -> [[f32; 4]; 16] {
    let (red, green) = (
        signed_channel(first_eight(block)),
        signed_channel(last_eight(block)),
    );
    core::array::from_fn(|texel| [red[texel], green[texel], 0.0, 1.0])
}

/// A sixteen-byte block read from its first bit, least significant first (Data Format 23.1).
struct Bits {
    bits: u128,
    at: u32,
}

impl Bits {
    const fn new(block: &[u8; 16]) -> Self {
        Self {
            bits: u128::from_le_bytes(*block),
            at: 0,
        }
    }

    /// The next `count` bits, the first read the least significant.
    fn take(&mut self, count: u32) -> u32 {
        if count == 0 {
            return 0;
        }
        let value = (self.bits >> self.at) as u32 & (u32::MAX >> (32 - count));
        self.at += count;
        value
    }
}

/// `BC7`'s and `BC6H`'s two-subset partitions, a bit a texel giving its subset, texel 0 the least
/// significant (Data Format table 110, Direct3D 11's `BC7` partition set for two subsets).
const PARTITIONS_2: [u16; 64] = [
    0xCCCC, 0x8888, 0xEEEE, 0xECC8, 0xC880, 0xFEEC, 0xFEC8, 0xEC80, 0xC800, 0xFFEC, 0xFE80, 0xE800,
    0xFFE8, 0xFF00, 0xFFF0, 0xF000, 0xF710, 0x008E, 0x7100, 0x08CE, 0x008C, 0x7310, 0x3100, 0x8CCE,
    0x088C, 0x3110, 0x6666, 0x366C, 0x17E8, 0x0FF0, 0x718E, 0x399C, 0xAAAA, 0xF0F0, 0x5A5A, 0x33CC,
    0x3C3C, 0x55AA, 0x9696, 0xA55A, 0x73CE, 0x13C8, 0x324C, 0x3BDC, 0x6996, 0xC33C, 0x9966, 0x0660,
    0x0272, 0x04E4, 0x4E40, 0x2720, 0xC936, 0x936C, 0x39C6, 0x639C, 0x9336, 0x9CC6, 0x817E, 0xE718,
    0xCCF0, 0x0FCC, 0x7744, 0xEE22,
];

/// `BC7`'s three-subset partitions, each texel's subset (Data Format table 111).
const PARTITIONS_3: [[u8; 16]; 64] = [
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 1, 2, 2, 2, 2],
    [0, 0, 0, 1, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 2, 1],
    [0, 0, 0, 0, 2, 0, 0, 1, 2, 2, 1, 1, 2, 2, 1, 1],
    [0, 2, 2, 2, 0, 0, 2, 2, 0, 0, 1, 1, 0, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2],
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 2, 2],
    [0, 0, 2, 2, 0, 0, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2],
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2],
    [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2],
    [0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 2],
    [0, 1, 2, 2, 0, 1, 2, 2, 0, 1, 2, 2, 0, 1, 2, 2],
    [0, 0, 1, 1, 0, 1, 1, 2, 1, 1, 2, 2, 1, 2, 2, 2],
    [0, 0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0, 2, 2, 2, 0],
    [0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 2, 1, 1, 2, 2],
    [0, 1, 1, 1, 0, 0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0],
    [0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2],
    [0, 0, 2, 2, 0, 0, 2, 2, 0, 0, 2, 2, 1, 1, 1, 1],
    [0, 1, 1, 1, 0, 1, 1, 1, 0, 2, 2, 2, 0, 2, 2, 2],
    [0, 0, 0, 1, 0, 0, 0, 1, 2, 2, 2, 1, 2, 2, 2, 1],
    [0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 2, 2, 0, 1, 2, 2],
    [0, 0, 0, 0, 1, 1, 0, 0, 2, 2, 1, 0, 2, 2, 1, 0],
    [0, 1, 2, 2, 0, 1, 2, 2, 0, 0, 1, 1, 0, 0, 0, 0],
    [0, 0, 1, 2, 0, 0, 1, 2, 1, 1, 2, 2, 2, 2, 2, 2],
    [0, 1, 1, 0, 1, 2, 2, 1, 1, 2, 2, 1, 0, 1, 1, 0],
    [0, 0, 0, 0, 0, 1, 1, 0, 1, 2, 2, 1, 1, 2, 2, 1],
    [0, 0, 2, 2, 1, 1, 0, 2, 1, 1, 0, 2, 0, 0, 2, 2],
    [0, 1, 1, 0, 0, 1, 1, 0, 2, 0, 0, 2, 2, 2, 2, 2],
    [0, 0, 1, 1, 0, 1, 2, 2, 0, 1, 2, 2, 0, 0, 1, 1],
    [0, 0, 0, 0, 2, 0, 0, 0, 2, 2, 1, 1, 2, 2, 2, 1],
    [0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 2, 2, 2],
    [0, 2, 2, 2, 0, 0, 2, 2, 0, 0, 1, 2, 0, 0, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 2, 0, 0, 2, 2, 0, 2, 2, 2],
    [0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0],
    [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0],
    [0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0],
    [0, 1, 2, 0, 2, 0, 1, 2, 1, 2, 0, 1, 0, 1, 2, 0],
    [0, 0, 1, 1, 2, 2, 0, 0, 1, 1, 2, 2, 0, 0, 1, 1],
    [0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 2, 1, 2, 1, 2, 1],
    [0, 0, 2, 2, 1, 1, 2, 2, 0, 0, 2, 2, 1, 1, 2, 2],
    [0, 0, 2, 2, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 1, 1],
    [0, 2, 2, 0, 1, 2, 2, 1, 0, 2, 2, 0, 1, 2, 2, 1],
    [0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 0, 1, 0, 1],
    [0, 0, 0, 0, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1],
    [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2],
    [0, 2, 2, 2, 0, 1, 1, 1, 0, 2, 2, 2, 0, 1, 1, 1],
    [0, 0, 0, 2, 1, 1, 1, 2, 0, 0, 0, 2, 1, 1, 1, 2],
    [0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2],
    [0, 2, 2, 2, 0, 1, 1, 1, 0, 1, 1, 1, 0, 2, 2, 2],
    [0, 0, 0, 2, 1, 1, 1, 2, 1, 1, 1, 2, 0, 0, 0, 2],
    [0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 2, 2],
    [0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 1, 2],
    [0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 0, 2, 2, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 2, 2],
    [0, 0, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 0, 0, 2, 2],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2],
    [0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1],
    [0, 2, 2, 2, 1, 2, 2, 2, 0, 2, 2, 2, 1, 2, 2, 2],
    [0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 1, 1, 1, 2, 0, 1, 1, 2, 2, 0, 1, 2, 2, 2, 0],
];

/// The second subset's anchor texel in each two-subset partition (Data Format table 112).
const ANCHORS_2_OF_2: [u8; 64] = [
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 2, 8, 2, 2, 8, 8, 15, 2, 8,
    2, 2, 8, 8, 2, 2, 15, 15, 6, 8, 2, 8, 15, 15, 2, 8, 2, 2, 2, 15, 15, 6, 6, 2, 6, 8, 15, 15, 2,
    2, 15, 15, 15, 15, 15, 2, 2, 15,
];

/// The second subset's anchor texel in each three-subset partition (Data Format table 113).
const ANCHORS_2_OF_3: [u8; 64] = [
    3, 3, 15, 15, 8, 3, 15, 15, 8, 8, 6, 6, 6, 5, 3, 3, 3, 3, 8, 15, 3, 3, 6, 10, 5, 8, 8, 6, 8, 5,
    15, 15, 8, 15, 3, 5, 6, 10, 8, 15, 15, 3, 15, 5, 15, 15, 15, 15, 3, 15, 5, 5, 5, 8, 5, 10, 5,
    10, 8, 13, 15, 12, 3, 3,
];

/// The third subset's anchor texel in each three-subset partition (Data Format table 114).
const ANCHORS_3_OF_3: [u8; 64] = [
    15, 8, 8, 3, 15, 15, 3, 8, 15, 15, 15, 15, 15, 15, 15, 8, 15, 8, 15, 3, 15, 8, 15, 8, 3, 15, 6,
    10, 15, 15, 10, 8, 15, 3, 15, 10, 10, 8, 9, 10, 6, 15, 8, 15, 3, 6, 6, 8, 15, 3, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 3, 15, 15, 8,
];

/// The interpolation weights out of 64 for two-, three- and four-bit indices (Data Format 23.3).
const WEIGHTS_2: [u32; 4] = [0, 21, 43, 64];
const WEIGHTS_3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const WEIGHTS_4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

const fn weights(bits: u32) -> &'static [u32] {
    match bits {
        2 => &WEIGHTS_2,
        3 => &WEIGHTS_3,
        _ => &WEIGHTS_4,
    }
}

/// Which subset texel `texel` is in, of `subsets` under partition `partition`.
fn subset_of(subsets: u32, partition: usize, texel: usize) -> usize {
    match subsets {
        2 => usize::from(PARTITIONS_2[partition] >> texel & 1),
        3 => usize::from(PARTITIONS_3[partition][texel]),
        _ => 0,
    }
}

/// Whether `texel` is an anchor - its index a bit short, the top one implied zero - of `subsets`
/// subsets under `partition`.
fn is_anchor(subsets: u32, partition: usize, texel: usize) -> bool {
    texel == 0
        || match subsets {
            2 => usize::from(ANCHORS_2_OF_2[partition]) == texel,
            3 => {
                usize::from(ANCHORS_2_OF_3[partition]) == texel
                    || usize::from(ANCHORS_3_OF_3[partition]) == texel
            }
            _ => false,
        }
}

/// Each texel's index of `bits` bits, its anchors' a bit short.
fn read_indices(reader: &mut Bits, bits: u32, (subsets, partition): (u32, usize)) -> [u32; 16] {
    core::array::from_fn(|texel| {
        let short = is_anchor(subsets, partition, texel);
        reader.take(bits - u32::from(short))
    })
}

/// One `BC7` mode's shape (Data Format table 109).
struct Bc7Mode {
    subsets: u32,
    partition_bits: u32,
    rotation_bits: u32,
    selection_bit: bool,
    colour_bits: u32,
    alpha_bits: u32,
    /// A P-bit for each endpoint.
    endpoint_p: bool,
    /// A P-bit shared by each subset's two endpoints.
    shared_p: bool,
    index_bits: u32,
    second_index_bits: u32,
}

const BC7_MODES: [Bc7Mode; 8] = [
    Bc7Mode {
        subsets: 3,
        partition_bits: 4,
        rotation_bits: 0,
        selection_bit: false,
        colour_bits: 4,
        alpha_bits: 0,
        endpoint_p: true,
        shared_p: false,
        index_bits: 3,
        second_index_bits: 0,
    },
    Bc7Mode {
        subsets: 2,
        partition_bits: 6,
        rotation_bits: 0,
        selection_bit: false,
        colour_bits: 6,
        alpha_bits: 0,
        endpoint_p: false,
        shared_p: true,
        index_bits: 3,
        second_index_bits: 0,
    },
    Bc7Mode {
        subsets: 3,
        partition_bits: 6,
        rotation_bits: 0,
        selection_bit: false,
        colour_bits: 5,
        alpha_bits: 0,
        endpoint_p: false,
        shared_p: false,
        index_bits: 2,
        second_index_bits: 0,
    },
    Bc7Mode {
        subsets: 2,
        partition_bits: 6,
        rotation_bits: 0,
        selection_bit: false,
        colour_bits: 7,
        alpha_bits: 0,
        endpoint_p: true,
        shared_p: false,
        index_bits: 2,
        second_index_bits: 0,
    },
    Bc7Mode {
        subsets: 1,
        partition_bits: 0,
        rotation_bits: 2,
        selection_bit: true,
        colour_bits: 5,
        alpha_bits: 6,
        endpoint_p: false,
        shared_p: false,
        index_bits: 2,
        second_index_bits: 3,
    },
    Bc7Mode {
        subsets: 1,
        partition_bits: 0,
        rotation_bits: 2,
        selection_bit: false,
        colour_bits: 7,
        alpha_bits: 8,
        endpoint_p: false,
        shared_p: false,
        index_bits: 2,
        second_index_bits: 2,
    },
    Bc7Mode {
        subsets: 1,
        partition_bits: 0,
        rotation_bits: 0,
        selection_bit: false,
        colour_bits: 7,
        alpha_bits: 7,
        endpoint_p: true,
        shared_p: false,
        index_bits: 4,
        second_index_bits: 0,
    },
    Bc7Mode {
        subsets: 2,
        partition_bits: 6,
        rotation_bits: 0,
        selection_bit: false,
        colour_bits: 5,
        alpha_bits: 5,
        endpoint_p: true,
        shared_p: false,
        index_bits: 2,
        second_index_bits: 0,
    },
];

/// A `BC7` block (Data Format 23.2-23.4). A block whose first byte is zero names no mode, and
/// decodes to transparent black.
#[must_use]
pub fn bc7(block: &[u8; 16]) -> Rgba8Block {
    let Some(number) = (0_usize..8).find(|&bit| block[0] >> bit & 1 == 1) else {
        return [[0; 4]; 16];
    };
    let mode = &BC7_MODES[number];
    let mut reader = Bits::new(block);
    reader.take(number as u32 + 1);
    let partition = reader.take(mode.partition_bits) as usize;
    let rotation = reader.take(mode.rotation_bits);
    let selection = mode.selection_bit && reader.take(1) == 1;
    let endpoints = (mode.subsets * 2) as usize;
    // endpoint, channel: red, green and blue for every endpoint, then alpha.
    let mut values = [[0_u32; 4]; 6];
    for channel in 0..3 {
        for value in values.iter_mut().take(endpoints) {
            value[channel] = reader.take(mode.colour_bits);
        }
    }
    for value in values.iter_mut().take(endpoints) {
        value[3] = reader.take(mode.alpha_bits);
    }
    let mut p_bits = [0_u32; 6];
    if mode.endpoint_p {
        for p in p_bits.iter_mut().take(endpoints) {
            *p = reader.take(1);
        }
    } else if mode.shared_p {
        for subset in 0..mode.subsets as usize {
            let p = reader.take(1);
            p_bits[2 * subset] = p;
            p_bits[2 * subset + 1] = p;
        }
    }
    let p_extra = u32::from(mode.endpoint_p || mode.shared_p);
    let mut colours = [[0_u8; 4]; 6];
    for (endpoint, colour) in colours.iter_mut().enumerate().take(endpoints) {
        for channel in 0..4 {
            let bits = if channel == 3 {
                mode.alpha_bits
            } else {
                mode.colour_bits
            };
            colour[channel] = if bits == 0 {
                255
            } else {
                let width = bits + p_extra;
                widen(
                    values[endpoint][channel] << p_extra | p_bits[endpoint],
                    width,
                )
            };
        }
    }
    let shape = (mode.subsets, partition);
    let primary = read_indices(&mut reader, mode.index_bits, shape);
    let secondary = if mode.second_index_bits > 0 {
        read_indices(&mut reader, mode.second_index_bits, (1, 0))
    } else {
        primary
    };
    let (colour_index, colour_bits, alpha_index, alpha_bits) = if mode.second_index_bits == 0 {
        (primary, mode.index_bits, primary, mode.index_bits)
    } else if selection {
        (secondary, mode.second_index_bits, primary, mode.index_bits)
    } else {
        (primary, mode.index_bits, secondary, mode.second_index_bits)
    };
    core::array::from_fn(|texel| {
        let subset = subset_of(mode.subsets, partition, texel);
        let (a, b) = (colours[2 * subset], colours[2 * subset + 1]);
        let interpolate = |channel: usize, index: u32, bits: u32| {
            let w = weights(bits)[index as usize];
            ((u32::from(a[channel]) * (64 - w) + u32::from(b[channel]) * w + 32) >> 6) as u8
        };
        let mut out = [
            interpolate(0, colour_index[texel], colour_bits),
            interpolate(1, colour_index[texel], colour_bits),
            interpolate(2, colour_index[texel], colour_bits),
            interpolate(3, alpha_index[texel], alpha_bits),
        ];
        // Rotation exchanges alpha with red, green or blue after interpolation.
        if rotation > 0 {
            out.swap(3, rotation as usize - 1);
        }
        out
    })
}

/// A `BC6H` endpoint field: one of the four endpoints' red, green or blue.
#[derive(Clone, Copy)]
enum Field {
    Rw,
    Gw,
    Bw,
    Rx,
    Gx,
    Bx,
    Ry,
    Gy,
    By,
    Rz,
    Gz,
    Bz,
}

/// One run of a `BC6H` header's bits: `count` bits into `field` from bit `shift` up, or, where
/// `reversed`, the first read its highest.
#[derive(Clone, Copy)]
struct Run {
    field: Field,
    shift: u32,
    count: u32,
    reversed: bool,
}

const fn run(field: Field, shift: u32, count: u32) -> Run {
    Run {
        field,
        shift,
        count,
        reversed: false,
    }
}

const fn reversed(field: Field, shift: u32, count: u32) -> Run {
    Run {
        field,
        shift,
        count,
        reversed: true,
    }
}

/// One `BC6H` mode (Microsoft, "BC6H Format", the mode table and bit layouts): its endpoint
/// precision, its deltas' precision for red, green and blue, whether the other endpoints are
/// deltas from the first, two subsets or one, and its header's bits after the mode bits.
struct Bc6Mode {
    precision: u32,
    deltas: [u32; 3],
    transformed: bool,
    subsets: u32,
    runs: &'static [Run],
}

use Field::{Bw, Bx, By, Bz, Gw, Gx, Gy, Gz, Rw, Rx, Ry, Rz};

const BC6_MODE_1: &[Run] = &[
    run(Gy, 4, 1),
    run(By, 4, 1),
    run(Bz, 4, 1),
    run(Rw, 0, 10),
    run(Gw, 0, 10),
    run(Bw, 0, 10),
    run(Rx, 0, 5),
    run(Gz, 4, 1),
    run(Gy, 0, 4),
    run(Gx, 0, 5),
    run(Bz, 0, 1),
    run(Gz, 0, 4),
    run(Bx, 0, 5),
    run(Bz, 1, 1),
    run(By, 0, 4),
    run(Ry, 0, 5),
    run(Bz, 2, 1),
    run(Rz, 0, 5),
    run(Bz, 3, 1),
];

const BC6_MODE_2: &[Run] = &[
    run(Gy, 5, 1),
    run(Gz, 4, 2),
    run(Rw, 0, 7),
    run(Bz, 0, 2),
    run(By, 4, 1),
    run(Gw, 0, 7),
    run(By, 5, 1),
    run(Bz, 2, 1),
    run(Gy, 4, 1),
    run(Bw, 0, 7),
    run(Bz, 3, 1),
    run(Bz, 5, 1),
    run(Bz, 4, 1),
    run(Rx, 0, 6),
    run(Gy, 0, 4),
    run(Gx, 0, 6),
    run(Gz, 0, 4),
    run(Bx, 0, 6),
    run(By, 0, 4),
    run(Ry, 0, 6),
    run(Rz, 0, 6),
];

const BC6_MODE_3: &[Run] = &[
    run(Rw, 0, 10),
    run(Gw, 0, 10),
    run(Bw, 0, 10),
    run(Rx, 0, 5),
    run(Rw, 10, 1),
    run(Gy, 0, 4),
    run(Gx, 0, 4),
    run(Gw, 10, 1),
    run(Bz, 0, 1),
    run(Gz, 0, 4),
    run(Bx, 0, 4),
    run(Bw, 10, 1),
    run(Bz, 1, 1),
    run(By, 0, 4),
    run(Ry, 0, 5),
    run(Bz, 2, 1),
    run(Rz, 0, 5),
    run(Bz, 3, 1),
];

const BC6_MODE_4: &[Run] = &[
    run(Rw, 0, 10),
    run(Gw, 0, 10),
    run(Bw, 0, 10),
    run(Rx, 0, 4),
    run(Rw, 10, 1),
    run(Gz, 4, 1),
    run(Gy, 0, 4),
    run(Gx, 0, 5),
    run(Gw, 10, 1),
    run(Gz, 0, 4),
    run(Bx, 0, 4),
    run(Bw, 10, 1),
    run(Bz, 1, 1),
    run(By, 0, 4),
    run(Ry, 0, 4),
    run(Bz, 0, 1),
    run(Bz, 2, 1),
    run(Rz, 0, 4),
    run(Gy, 4, 1),
    run(Bz, 3, 1),
];

const BC6_MODE_5: &[Run] = &[
    run(Rw, 0, 10),
    run(Gw, 0, 10),
    run(Bw, 0, 10),
    run(Rx, 0, 4),
    run(Rw, 10, 1),
    run(By, 4, 1),
    run(Gy, 0, 4),
    run(Gx, 0, 4),
    run(Gw, 10, 1),
    run(Bz, 0, 1),
    run(Gz, 0, 4),
    run(Bx, 0, 5),
    run(Bw, 10, 1),
    run(By, 0, 4),
    run(Ry, 0, 4),
    run(Bz, 1, 2),
    run(Rz, 0, 4),
    run(Bz, 4, 1),
    run(Bz, 3, 1),
];

const BC6_MODE_6: &[Run] = &[
    run(Rw, 0, 9),
    run(By, 4, 1),
    run(Gw, 0, 9),
    run(Gy, 4, 1),
    run(Bw, 0, 9),
    run(Bz, 4, 1),
    run(Rx, 0, 5),
    run(Gz, 4, 1),
    run(Gy, 0, 4),
    run(Gx, 0, 5),
    run(Bz, 0, 1),
    run(Gz, 0, 4),
    run(Bx, 0, 5),
    run(Bz, 1, 1),
    run(By, 0, 4),
    run(Ry, 0, 5),
    run(Bz, 2, 1),
    run(Rz, 0, 5),
    run(Bz, 3, 1),
];

const BC6_MODE_7: &[Run] = &[
    run(Rw, 0, 8),
    run(Gz, 4, 1),
    run(By, 4, 1),
    run(Gw, 0, 8),
    run(Bz, 2, 1),
    run(Gy, 4, 1),
    run(Bw, 0, 8),
    run(Bz, 3, 2),
    run(Rx, 0, 6),
    run(Gy, 0, 4),
    run(Gx, 0, 5),
    run(Bz, 0, 1),
    run(Gz, 0, 4),
    run(Bx, 0, 5),
    run(Bz, 1, 1),
    run(By, 0, 4),
    run(Ry, 0, 6),
    run(Rz, 0, 6),
];

const BC6_MODE_8: &[Run] = &[
    run(Rw, 0, 8),
    run(Bz, 0, 1),
    run(By, 4, 1),
    run(Gw, 0, 8),
    run(Gy, 5, 1),
    run(Gy, 4, 1),
    run(Bw, 0, 8),
    run(Gz, 5, 1),
    run(Bz, 4, 1),
    run(Rx, 0, 5),
    run(Gz, 4, 1),
    run(Gy, 0, 4),
    run(Gx, 0, 6),
    run(Gz, 0, 4),
    run(Bx, 0, 5),
    run(Bz, 1, 1),
    run(By, 0, 4),
    run(Ry, 0, 5),
    run(Bz, 2, 1),
    run(Rz, 0, 5),
    run(Bz, 3, 1),
];

const BC6_MODE_9: &[Run] = &[
    run(Rw, 0, 8),
    run(Bz, 1, 1),
    run(By, 4, 1),
    run(Gw, 0, 8),
    run(By, 5, 1),
    run(Gy, 4, 1),
    run(Bw, 0, 8),
    run(Bz, 5, 1),
    run(Bz, 4, 1),
    run(Rx, 0, 5),
    run(Gz, 4, 1),
    run(Gy, 0, 4),
    run(Gx, 0, 5),
    run(Bz, 0, 1),
    run(Gz, 0, 4),
    run(Bx, 0, 6),
    run(By, 0, 4),
    run(Ry, 0, 5),
    run(Bz, 2, 1),
    run(Rz, 0, 5),
    run(Bz, 3, 1),
];

const BC6_MODE_10: &[Run] = &[
    run(Rw, 0, 6),
    run(Gz, 4, 1),
    run(Bz, 0, 2),
    run(By, 4, 1),
    run(Gw, 0, 6),
    run(Gy, 5, 1),
    run(By, 5, 1),
    run(Bz, 2, 1),
    run(Gy, 4, 1),
    run(Bw, 0, 6),
    run(Gz, 5, 1),
    run(Bz, 3, 1),
    run(Bz, 5, 1),
    run(Bz, 4, 1),
    run(Rx, 0, 6),
    run(Gy, 0, 4),
    run(Gx, 0, 6),
    run(Gz, 0, 4),
    run(Bx, 0, 6),
    run(By, 0, 4),
    run(Ry, 0, 6),
    run(Rz, 0, 6),
];

const BC6_MODE_11: &[Run] = &[
    run(Rw, 0, 10),
    run(Gw, 0, 10),
    run(Bw, 0, 10),
    run(Rx, 0, 10),
    run(Gx, 0, 10),
    run(Bx, 0, 10),
];

const BC6_MODE_12: &[Run] = &[
    run(Rw, 0, 10),
    run(Gw, 0, 10),
    run(Bw, 0, 10),
    run(Rx, 0, 9),
    run(Rw, 10, 1),
    run(Gx, 0, 9),
    run(Gw, 10, 1),
    run(Bx, 0, 9),
    run(Bw, 10, 1),
];

const BC6_MODE_13: &[Run] = &[
    run(Rw, 0, 10),
    run(Gw, 0, 10),
    run(Bw, 0, 10),
    run(Rx, 0, 8),
    reversed(Rw, 10, 2),
    run(Gx, 0, 8),
    reversed(Gw, 10, 2),
    run(Bx, 0, 8),
    reversed(Bw, 10, 2),
];

const BC6_MODE_14: &[Run] = &[
    run(Rw, 0, 10),
    run(Gw, 0, 10),
    run(Bw, 0, 10),
    run(Rx, 0, 4),
    reversed(Rw, 10, 6),
    run(Gx, 0, 4),
    reversed(Gw, 10, 6),
    run(Bx, 0, 4),
    reversed(Bw, 10, 6),
];

/// `BC6H`'s modes by the value of their mode bits: two bits for the first two, five for the rest.
fn bc6_mode(value: u32) -> Option<Bc6Mode> {
    let two = |precision, deltas, transformed, runs| Bc6Mode {
        precision,
        deltas,
        transformed,
        subsets: 2,
        runs,
    };
    let one = |precision, delta, runs| Bc6Mode {
        precision,
        deltas: [delta; 3],
        transformed: precision != 10,
        subsets: 1,
        runs,
    };
    Some(match value {
        0 => two(10, [5, 5, 5], true, BC6_MODE_1),
        1 => two(7, [6, 6, 6], true, BC6_MODE_2),
        2 => two(11, [5, 4, 4], true, BC6_MODE_3),
        6 => two(11, [4, 5, 4], true, BC6_MODE_4),
        10 => two(11, [4, 4, 5], true, BC6_MODE_5),
        14 => two(9, [5, 5, 5], true, BC6_MODE_6),
        18 => two(8, [6, 5, 5], true, BC6_MODE_7),
        22 => two(8, [5, 6, 5], true, BC6_MODE_8),
        26 => two(8, [5, 5, 6], true, BC6_MODE_9),
        30 => two(6, [6, 6, 6], false, BC6_MODE_10),
        3 => one(10, 10, BC6_MODE_11),
        7 => one(11, 9, BC6_MODE_12),
        11 => one(12, 8, BC6_MODE_13),
        15 => one(16, 4, BC6_MODE_14),
        _ => return None,
    })
}

/// `value`'s low `bits` bits as a two's-complement number.
const fn sign_extend(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

/// An endpoint component of `bits` bits as the 16-bit value interpolation works in (Microsoft,
/// "BC6H Format", `unquantize`).
const fn unquantize(value: i32, bits: u32, signed: bool) -> i32 {
    if signed {
        if bits >= 16 {
            return value;
        }
        let (negative, magnitude) = (value < 0, value.abs());
        let unquantized = if magnitude == 0 {
            0
        } else if magnitude >= (1 << (bits - 1)) - 1 {
            0x7FFF
        } else {
            ((magnitude << 15) + 0x4000) >> (bits - 1)
        };
        if negative { -unquantized } else { unquantized }
    } else if bits >= 15 {
        value
    } else if value == 0 {
        0
    } else if value == (1 << bits) - 1 {
        0xFFFF
    } else {
        ((value << 16) + 0x8000) >> bits
    }
}

/// An interpolated value scaled to a half float's bits (Microsoft, "BC6H Format", `finish_unquantize`).
const fn finish(value: i32, signed: bool) -> u16 {
    if signed {
        if value < 0 {
            0x8000 | (((-value) * 31) >> 5) as u16
        } else {
            ((value * 31) >> 5) as u16
        }
    } else {
        ((value * 31) >> 6) as u16
    }
}

/// A `BC6H` block (Data Format 23.5-23.6), unsigned (`BC6H_UFLOAT`) or `signed` (`BC6H_SFLOAT`), as
/// red, green and blue half floats. A reserved mode decodes to zero.
#[must_use]
pub fn bc6h(block: &[u8; 16], signed: bool) -> HalfBlock {
    let mut reader = Bits::new(block);
    let low = reader.take(2);
    let value = if low < 2 {
        low
    } else {
        low | reader.take(3) << 2
    };
    let Some(mode) = bc6_mode(value) else {
        return [[0; 3]; 16];
    };
    let mut fields = [0_u32; 12];
    for part in mode.runs {
        let mut bits = reader.take(part.count);
        if part.reversed {
            bits = bits.reverse_bits() >> (32 - part.count);
        }
        fields[part.field as usize] |= bits << part.shift;
    }
    let partition = if mode.subsets == 2 {
        reader.take(5) as usize
    } else {
        0
    };
    // Endpoints as `[endpoint][channel]`: w and x the first subset's, y and z the second's.
    let mut endpoints = [[0_i32; 3]; 4];
    let mask = (1_u32 << mode.precision) - 1;
    for channel in 0..3 {
        let base = fields[channel];
        endpoints[0][channel] = if signed {
            sign_extend(base, mode.precision)
        } else {
            base as i32
        };
        for endpoint in 1..4 {
            let raw = fields[3 * endpoint + channel];
            let value = if mode.transformed {
                let delta = sign_extend(raw, mode.deltas[channel]);
                (base as i32).wrapping_add(delta) as u32 & mask
            } else {
                raw
            };
            endpoints[endpoint][channel] = if signed {
                sign_extend(value, mode.precision)
            } else {
                value as i32
            };
        }
    }
    let unquantized = endpoints.map(|e| e.map(|c| unquantize(c, mode.precision, signed)));
    let bits = if mode.subsets == 2 { 3 } else { 4 };
    let indices = read_indices(&mut reader, bits, (mode.subsets, partition));
    core::array::from_fn(|texel| {
        let subset = subset_of(mode.subsets, partition, texel);
        let w = weights(bits)[indices[texel] as usize] as i32;
        let (a, b) = (unquantized[2 * subset], unquantized[2 * subset + 1]);
        [0, 1, 2].map(|c| finish((a[c] * (64 - w) + b[c] * w + 32) >> 6, signed))
    })
}

#[cfg(test)]
mod tests {
    use super::{bc1, bc2, bc3, bc4_snorm, bc4_unorm, bc5_unorm, bc6h, bc7};

    /// A four-colour `BC1` block reads its endpoints and their thirds; a three-colour one its
    /// midpoint and, at index 3, transparent black (Data Format 21.1).
    #[test]
    fn bc1_reads_both_of_its_forms() {
        // Red over blue, the four indices 0, 1, 2, 3 along the first row.
        let four = bc1(&[0x00, 0xF8, 0x1F, 0x00, 0b1110_0100, 0, 0, 0]);
        assert_eq!(four[0], [255, 0, 0, 255]);
        assert_eq!(four[1], [0, 0, 255, 255]);
        assert_eq!(four[2], [170, 0, 85, 255]);
        assert_eq!(four[3], [85, 0, 170, 255]);
        // The same endpoints the other way round: three colours and a hole.
        let three = bc1(&[0x1F, 0x00, 0x00, 0xF8, 0b1110_0100, 0, 0, 0]);
        assert_eq!(three[2], [128, 0, 128, 255]);
        assert_eq!(three[3], [0, 0, 0, 0]);
    }

    /// `BC2` and `BC3` decode their colour in the four-colour form whatever the endpoints' order,
    /// and carry alpha of their own (Data Format 21.2, 21.3).
    #[test]
    fn bc2_and_bc3_carry_their_own_alpha() {
        let colour = [0x1F, 0x00, 0x00, 0xF8, 0b1110_0100, 0, 0, 0];
        let mut block = [0_u8; 16];
        block[..8].copy_from_slice(&[0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE]);
        block[8..].copy_from_slice(&colour);
        let texels = bc2(&block);
        assert_eq!(
            texels[3],
            [170, 0, 85, 0x33],
            "index 3 is a colour, alpha 3 widened"
        );
        assert_eq!(texels[15][3], 255);
        // Alpha endpoints 255 and 0, the second texel's index 1.
        block[..8].copy_from_slice(&[255, 0, 0b0000_1000, 0, 0, 0, 0, 0]);
        let texels = bc3(&block);
        assert_eq!((texels[0][3], texels[1][3]), (255, 0));
    }

    /// A one-channel block in its six-value form reaches both extremes (Data Format 22.1, 22.2).
    #[test]
    fn a_one_channel_block_reaches_its_extremes() {
        // Endpoints 0 then 255 (six-value form), the first texels' indices 6 and 7.
        let unsigned = bc4_unorm(&[0, 255, 0b0011_1110, 0, 0, 0, 0, 0]);
        assert_eq!(unsigned[0], [0, 0, 0, 255], "index 6 is zero");
        assert_eq!(unsigned[1], [255, 0, 0, 255], "index 7 is full scale");
        // Signed endpoints -128 and -127 both read as -1.
        let signed = bc4_snorm(&[0x80, 0x81, 0, 0, 0, 0, 0, 0]);
        assert!((signed[0][0] + 1.0).abs() < 1e-6, "-128 is -1, as -127 is");
        let mut block = [0_u8; 16];
        block[..2].copy_from_slice(&[9, 9]);
        block[8..10].copy_from_slice(&[7, 7]);
        let two = bc5_unorm(&block);
        assert_eq!(two[5], [9, 7, 0, 255]);
    }

    /// `BC7` mode 6 - one subset, seven-bit colour and alpha with a P-bit each - interpolates
    /// between its endpoints by four-bit indices; a block of no mode is transparent black.
    #[test]
    fn bc7_mode_6_interpolates_its_endpoints() {
        // Mode 6, endpoint 0 all 0 and endpoint 1 all 127, P-bits 0 and 1: 0 and 255.
        let mut bits: u128 = 1 << 6;
        let mut at = 7;
        for _ in 0..4 {
            bits |= 0 << at;
            bits |= 127 << (at + 7);
            at += 14;
        }
        bits |= 1 << (at + 1);
        at += 2;
        // Texel 0 index 0 (three bits), texel 1 index 15.
        bits |= 15 << (at + 3);
        let texels = bc7(&bits.to_le_bytes());
        assert_eq!(texels[0], [0, 0, 0, 0]);
        assert_eq!(texels[1], [255, 255, 255, 255]);
        assert_eq!(bc7(&[0; 16]), [[0; 4]; 16]);
    }

    /// `BC6H` mode 11 - one subset, ten-bit endpoints as they are - finishes each endpoint to the
    /// half float it unquantizes to: an endpoint of full scale is the largest finite unsigned
    /// value `0x7BFF`, and zero is zero.
    #[test]
    fn bc6h_mode_11_finishes_its_endpoints() {
        // Mode bits 00011, endpoint w zero, endpoint x 0x3FF in each channel, every index 0 but
        // the second texel's 15.
        let mut bits: u128 = 0b00011;
        bits |= ((0x3FF_u128) | 0x3FF << 10 | 0x3FF << 20) << 35;
        bits |= 15 << (65 + 3);
        let texels = bc6h(&bits.to_le_bytes(), false);
        assert_eq!(texels[0], [0, 0, 0]);
        assert_eq!(texels[1], [0x7BFF, 0x7BFF, 0x7BFF]);
        // A reserved mode is zero.
        assert_eq!(
            bc6h(
                &[0b10011, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                false
            )[0],
            [0; 3]
        );
    }
}
