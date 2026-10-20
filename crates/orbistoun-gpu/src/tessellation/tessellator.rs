//! The fixed-function tessellator: tessellation factors in, domain points and the primitives
//! joining them out (D928).
//!
//! A Rust port of `CHWTessellator`, the D3D11 functional specification's reference for what the
//! tessellator stage must produce ("Microsoft D3D11 Fixed Function Tessellator Reference",
//! Microsoft, MIT), as Mesa carries it in `src/gallium/auxiliary/tessellator/tessellator.cpp`.
//! Each function names the lines it follows. The arithmetic is the reference's 16.16 fixed point,
//! unsigned and wrapping where the reference's is, so a point lands on the same 16-bit fraction
//! and the points and indices come out in the reference's order.
//!
//! The reference's `CHLSLTessellator` is not ported: it is code the HLSL compiler places in a hull
//! shader, and a console hull shader that wants it carries it in its own program.
//!
//! What the console's tessellator produces is measured, not assumed to be this: the factor
//! rounding, the point positions and the order points and primitives come out in are each a row
//! of obSCEne's tessellation census (D928). Until those rows exist this is the published contract
//! the hardware is built to, and the caller says so when it uses it.

use super::{OutputTopology, Partitioning};

/// The largest factor an odd partitioning keeps (`tessellator.hpp:62`).
const MAX_ODD_FACTOR: f32 = 63.0;
/// The smallest factor an even partitioning keeps (`tessellator.hpp:63`).
const MIN_EVEN_FACTOR: f32 = 2.0;
/// The largest factor an even or integer partitioning keeps (`tessellator.hpp:64`).
const MAX_EVEN_FACTOR: f32 = 64.0;
/// The smallest factor an odd or integer partitioning keeps (`tessellator.hpp:61`).
const MIN_ODD_FACTOR: f32 = 1.0;
/// An isoline's density bounds (`tessellator.hpp:66-67`).
const MIN_DENSITY: f32 = 1.0;
const MAX_DENSITY: f32 = 64.0;

/// The most points and indices one patch makes (`tessellator.hpp:71-72`).
pub const MAX_POINTS: usize = 65 * 65;
/// See [`MAX_POINTS`].
pub const MAX_INDICES: usize = 64 * 64 * 2 * 3;

/// 16.16 fixed point (`tessellator.cpp:343-351`).
type Fxp = u32;
const FRACTION_BITS: u32 = 16;
const FRACTION_MASK: Fxp = 0x0000_ffff;
const INTEGER_MASK: Fxp = 0x7fff_0000;
const ONE: Fxp = 1 << FRACTION_BITS;
const ONE_THIRD: Fxp = 0x0000_5555;
const TWO_THIRDS: Fxp = 0x0000_aaaa;
const ONE_HALF: Fxp = 0x0000_8000;

/// 2^-16, the least positive fraction, and one plus half of it (`tessellator.cpp:606-607`).
const EPSILON: f32 = 1.0 / 65536.0;
const MIN_ODD_PLUS_HALF_EPSILON: f32 = MIN_ODD_FACTOR + EPSILON / 2.0;

/// `1 / n` in 16.16 for `n` segments, entry 0 unused (`tessellator.cpp:361-380`).
const RECIPROCAL: [Fxp; 65] = [
    0xffff_ffff,
    0x10000,
    0x8000,
    0x5555,
    0x4000,
    0x3333,
    0x2aab,
    0x2492,
    0x2000,
    0x1c72,
    0x199a,
    0x1746,
    0x1555,
    0x13b1,
    0x1249,
    0x1111,
    0x1000,
    0xf0f,
    0xe39,
    0xd79,
    0xccd,
    0xc31,
    0xba3,
    0xb21,
    0xaab,
    0xa3d,
    0x9d9,
    0x97b,
    0x925,
    0x8d4,
    0x889,
    0x842,
    0x800,
    0x7c2,
    0x788,
    0x750,
    0x71c,
    0x6eb,
    0x6bd,
    0x690,
    0x666,
    0x63e,
    0x618,
    0x5f4,
    0x5d1,
    0x5b0,
    0x591,
    0x572,
    0x555,
    0x539,
    0x51f,
    0x505,
    0x4ec,
    0x4d5,
    0x4be,
    0x4a8,
    0x492,
    0x47e,
    0x46a,
    0x457,
    0x444,
    0x432,
    0x421,
    0x410,
    0x400,
];

/// Whether a float is a NaN (`tess_isNaN`, `tessellator.cpp:46-53`).
fn is_nan(value: f32) -> bool {
    let bits = value.to_bits();
    bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0
}

/// A denormal flushed to a zero of its sign (`tess_flush`, `tessellator.cpp:58-73`).
fn flush(value: f32) -> f32 {
    let bits = value.to_bits();
    if bits & 0x7fff_ffff < 0x0080_0000 {
        f32::from_bits(bits & 0x8000_0000)
    } else {
        value
    }
}

/// IEEE 754R minimum: a NaN second operand gives the first (`tess_fmin`, `tessellator.cpp:78-93`).
fn fmin(a: f32, b: f32) -> f32 {
    let (fa, fb) = (flush(a), flush(b));
    if is_nan(fb) {
        a
    } else if fa == 0.0 && fb == 0.0 {
        if fa.to_bits() & 0x8000_0000 != 0 {
            a
        } else {
            b
        }
    } else if fa < fb {
        a
    } else {
        b
    }
}

/// IEEE 754R maximum: a NaN second operand gives the first (`tess_fmax`, `tessellator.cpp:98-114`).
fn fmax(a: f32, b: f32) -> f32 {
    let (fa, fb) = (flush(a), flush(b));
    if is_nan(fb) {
        a
    } else if fa == 0.0 && fb == 0.0 {
        if fb.to_bits() & 0x8000_0000 != 0 {
            a
        } else {
            b
        }
    } else if fa >= fb {
        a
    } else {
        b
    }
}

/// A float to unsigned 15.16 fixed point, rounding to nearest-even at the last kept bit
/// (`floatToIDotF<15, 16, false>`, `tessellator.cpp:139-340`). NaN and anything negative give 0;
/// anything too large gives all ones.
fn float_to_fixed(input: f32) -> Fxp {
    const MANTISSA_BITS: i32 = 23;
    const BIAS: i32 = 127;
    const HIDDEN: i32 = 1 << MANTISSA_BITS;
    const MANTISSA_MASK: i32 = HIDDEN - 1;
    const EXPONENT_MASK: i32 = 0xff << MANTISSA_BITS;
    const SIGN: i32 = i32::MIN;
    const INTEGER_BITS: i32 = 15;
    const FRACTION: i32 = 16;
    // Unsigned: 2^15 - 2^-16 is the largest value; `iShift` is negative, so nothing comes off it.
    const MAX_POSITIVE: i32 = (BIAS + INTEGER_BITS) << MANTISSA_BITS;
    const MAX_NEGATIVE: i32 = 0;
    const MIN_RESULT: i32 = 0;
    const MAX_RESULT: i32 = !MIN_RESULT;

    let mut output = input.to_bits() as i32;
    let exponent = ((output & EXPONENT_MASK) >> MANTISSA_BITS) - BIAS;
    let negative = output & SIGN != 0;
    let output = if exponent == BIAS + 1 && output & MANTISSA_MASK != 0 {
        0
    } else if !negative && output >= MAX_POSITIVE {
        MAX_RESULT
    } else if negative && (output & !SIGN) >= MAX_NEGATIVE {
        MIN_RESULT
    } else if exponent < -FRACTION - 1 {
        0
    } else {
        output = (output & MANTISSA_MASK) | HIDDEN;
        let extra = MANTISSA_BITS - FRACTION - exponent;
        if extra >= 0 {
            let lsb = 1 << extra;
            let extra_mask = lsb - 1;
            let half = lsb >> 1;
            if output & lsb != 0 || (output & extra_mask) > half {
                output += half;
            }
            output >> extra
        } else {
            output << -extra
        }
    };
    output as Fxp
}

/// 16.16 fixed point to a float (`fixedToFloat`, `tessellator.cpp:396-400`). Every value the
/// tessellator places, at most one with sixteen fraction bits, converts exactly.
fn fixed_to_float(input: Fxp) -> f32 {
    // Both halves are sixteen bits, which a float holds exactly.
    f32::from((input >> FRACTION_BITS) as u16) + f32::from((input & FRACTION_MASK) as u16) / 65536.0
}

/// Whether a float's integer part is even (`isEven`, `tessellator.cpp:405-408`).
fn is_even(input: f32) -> bool {
    (input as i32) & 1 == 0
}

/// Whether a factor is exactly one (`FLOAT_ONE == factor`, `tessellator.cpp:651`, `:1156`). It
/// has been clamped to at least one by then, so its bits are one's or it is not.
fn is_one(input: f32) -> bool {
    input.to_bits() == 1.0f32.to_bits()
}

/// `fxpCeil` and `fxpFloor` (`tessellator.cpp:413-428`).
fn fxp_ceil(input: Fxp) -> Fxp {
    if input & FRACTION_MASK != 0 {
        (input & INTEGER_MASK).wrapping_add(ONE)
    } else {
        input
    }
}

fn fxp_floor(input: Fxp) -> Fxp {
    input & INTEGER_MASK
}

/// The value without its highest set bit among the eight below the byte boundary that holds it
/// (`RemoveMSB`, `tessellator.cpp:1696-1703`).
fn remove_msb(value: i32) -> i32 {
    let value = value as u32;
    let mut check: u32 = if value <= 0x0000_ffff {
        if value <= 0x0000_00ff { 0x80 } else { 0x8000 }
    } else if value <= 0x00ff_ffff {
        0x0080_0000
    } else {
        0x8000_0000
    };
    for _ in 0..8 {
        if value & check != 0 {
            return (value & !check) as i32;
        }
        check >>= 1;
    }
    0
}

/// Which parity a factor is tessellated with (`TESSELLATOR_PARITY`, `tessellator.hpp:169-173`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Parity {
    Even,
    Odd,
}

/// What every point placed along one factor shares (`TESS_FACTOR_CONTEXT`, `tessellator.hpp:191-198`).
#[derive(Debug, Clone, Copy, Default)]
struct FactorContext {
    inverse_floor_segments: Fxp,
    inverse_ceil_segments: Fxp,
    half_fraction: Fxp,
    half_points: i32,
    split_point_on_floor: i32,
}

/// An isoline patch's factors as processed (`PROCESSED_TESS_FACTORS_ISOLINE`).
#[derive(Debug, Clone, Copy)]
struct IsolineFactors {
    density_parity: Parity,
    detail_parity: Parity,
    density: FactorContext,
    detail: FactorContext,
    points_per_line: i32,
    lines: i32,
}

/// A triangle patch's factors as processed (`PROCESSED_TESS_FACTORS_TRI`).
#[derive(Debug, Clone, Copy)]
struct TriangleFactors {
    outside_parity: [Parity; 3],
    inside_parity: Parity,
    outside_context: [FactorContext; 3],
    inside_context: FactorContext,
    outside_points: [i32; 3],
    inside_points: i32,
    inside_base: i32,
}

/// A quad patch's factors as processed (`PROCESSED_TESS_FACTORS_QUAD`).
#[derive(Debug, Clone, Copy)]
struct QuadFactors {
    outside_parity: [Parity; 4],
    inside_parity: [Parity; 2],
    outside_context: [FactorContext; 4],
    inside_context: [FactorContext; 2],
    outside_points: [i32; 4],
    inside_points: [i32; 2],
    inside_base: i32,
}

/// What processing a patch's factors decided (`bPatchCulled`, `bJustDoMinimumTessFactor`).
enum Processed<T> {
    Culled,
    Minimum,
    Full(T),
}

/// The index rewrite stitching the last side of a ring (`INDEX_PATCH_CONTEXT`, `tessellator.hpp:325-334`).
#[derive(Debug, Clone, Copy, Default)]
struct IndexPatch {
    inside_delta: i32,
    inside_bad: i32,
    inside_replacement: i32,
    outside_base: i32,
    outside_delta: i32,
    outside_bad: i32,
    outside_replacement: i32,
}

/// The index rewrite for an even quad's middle row (`INDEX_PATCH_CONTEXT2`, `tessellator.hpp:342-348`).
#[derive(Debug, Clone, Copy, Default)]
struct IndexPatch2 {
    base_to_invert: i32,
    inversion_end: i32,
    corner_bad: i32,
    corner_replacement: i32,
}

/// How the diagonals of a regular stitch run (`DIAGONALS`, `tessellator.hpp:299-304`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Diagonals {
    InsideToOutside,
    InsideToOutsideExceptMiddle,
    Mirrored,
}

/// The tessellator, configured for one partitioning and output topology; it keeps its storage
/// between patches (`CHWTessellator`).
#[derive(Debug, Clone)]
pub struct Tessellator {
    partitioning: Partitioning,
    output: OutputTopology,
    /// The parity the next point placement uses; the reference threads it through as state
    /// (`m_parity`), and so does this.
    parity: Parity,
    original_parity: Parity,
    points: Vec<[f32; 2]>,
    indices: Vec<u32>,
    point_count: i32,
    index_count: i32,
    patched: bool,
    patched2: bool,
    patch: IndexPatch,
    patch2: IndexPatch2,
}

impl Tessellator {
    /// A tessellator for `partitioning` producing `output` (`Init`, `tessellator.cpp:459-489`).
    #[must_use]
    pub fn new(partitioning: Partitioning, output: OutputTopology) -> Self {
        // The reference leaves the parity of an integer partitioning unset: every factor sets
        // its own before it is read.
        let parity = match partitioning {
            Partitioning::FractionalOdd => Parity::Odd,
            Partitioning::Integer | Partitioning::Pow2 | Partitioning::FractionalEven => {
                Parity::Even
            }
        };
        Self {
            partitioning,
            output,
            parity,
            original_parity: parity,
            points: vec![[0.0; 2]; MAX_POINTS],
            indices: vec![0; MAX_INDICES],
            point_count: 0,
            index_count: 0,
            patched: false,
            patched2: false,
            patch: IndexPatch::default(),
            patch2: IndexPatch2::default(),
        }
    }

    /// The domain points of the last patch, each `(u, v)`; a triangle's `w` is `1 - u - v`.
    #[must_use]
    pub fn points(&self) -> &[[f32; 2]] {
        &self.points[..self.point_count as usize]
    }

    /// The last patch's primitives as indices into [`Self::points`]: one per point, two per line,
    /// or three per triangle in the configured winding.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices[..self.index_count as usize]
    }

    fn odd(&self) -> bool {
        self.parity == Parity::Odd
    }

    /// Integer and power-of-two partitioning tessellate alike (`HWIntegerPartitioning`).
    fn integer_partitioning(&self) -> bool {
        matches!(
            self.partitioning,
            Partitioning::Integer | Partitioning::Pow2
        )
    }

    /// The bounds the patch's edge factors are clamped to (`tessellator.cpp:570-588`).
    fn bounds(&self) -> (f32, f32) {
        match self.partitioning {
            Partitioning::Integer | Partitioning::Pow2 => (MIN_ODD_FACTOR, MAX_EVEN_FACTOR),
            Partitioning::FractionalEven => (MIN_EVEN_FACTOR, MAX_EVEN_FACTOR),
            Partitioning::FractionalOdd => (MIN_ODD_FACTOR, MAX_ODD_FACTOR),
        }
    }

    fn clear(&mut self) {
        self.point_count = 0;
        self.index_count = 0;
    }

    /// An isoline patch: `density` lines of `detail` segments, the factors in the reference's
    /// order (`TessellateIsoLineDomain`, `tessellator.cpp:1430-1442`).
    pub fn isoline(&mut self, density: f32, detail: f32) {
        let Some(factors) = self.isoline_factors(density, detail) else {
            self.clear();
            return;
        };
        self.isoline_points(&factors);
        self.isoline_connectivity(&factors);
    }

    /// `IsoLineProcessTessFactors`, `tessellator.cpp:1447-1533`.
    fn isoline_factors(&mut self, density: f32, detail: f32) -> Option<IsolineFactors> {
        // A NaN fails the comparison and culls.
        if density.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            || detail.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        {
            return None;
        }
        let (lower, upper) = self.bounds();
        let mut density = fmin(MAX_DENSITY, fmax(MIN_DENSITY, density));
        let mut detail = fmin(upper, fmax(lower, detail));
        self.clear();
        let detail_parity = if self.integer_partitioning() {
            detail = detail.ceil();
            if is_even(detail) {
                Parity::Even
            } else {
                Parity::Odd
            }
        } else {
            self.original_parity
        };
        let fixed_detail = float_to_fixed(detail);
        self.parity = detail_parity;
        let detail_context = self.factor_context(fixed_detail);
        let points_per_line = self.points_for_factor(fixed_detail);
        // The density is tessellated as an integer whatever the partitioning; the reference's
        // override of the partitioning reaches nothing past this point.
        density = density.ceil();
        let density_parity = if is_even(density) {
            Parity::Even
        } else {
            Parity::Odd
        };
        self.parity = density_parity;
        let fixed_density = float_to_fixed(density);
        let density_context = self.factor_context(fixed_density);
        // The last line, at v = 1, is not drawn.
        let lines = self.points_for_factor(fixed_density) - 1;
        self.point_count = points_per_line * lines;
        self.index_count = if self.output == OutputTopology::Point {
            self.point_count
        } else {
            lines * (points_per_line - 1) * 2
        };
        Some(IsolineFactors {
            density_parity,
            detail_parity,
            density: density_context,
            detail: detail_context,
            points_per_line,
            lines,
        })
    }

    /// `IsoLineGeneratePoints`, `tessellator.cpp:1538-1555`.
    fn isoline_points(&mut self, factors: &IsolineFactors) {
        let mut offset = 0;
        for line in 0..factors.lines {
            for point in 0..factors.points_per_line {
                self.parity = factors.density_parity;
                let v = self.place_point(&factors.density, line);
                self.parity = factors.detail_parity;
                let u = self.place_point(&factors.detail, point);
                self.define_point(u, v, offset);
                offset += 1;
            }
        }
    }

    /// `IsoLineGenerateConnectivity`, `tessellator.cpp:1560-1588`.
    fn isoline_connectivity(&mut self, factors: &IsolineFactors) {
        let mut point_offset = 0;
        let mut index_offset = 0;
        for _ in 0..factors.lines {
            for point in 0..factors.points_per_line {
                if self.output == OutputTopology::Point {
                    self.define_index(point_offset, index_offset);
                    index_offset += 1;
                } else if point > 0 {
                    self.define_index(point_offset - 1, index_offset);
                    self.define_index(point_offset, index_offset + 1);
                    index_offset += 2;
                }
                point_offset += 1;
            }
        }
    }

    /// A triangle patch: the factors of the edges where `u`, `v` and `w` are zero, and the inside
    /// one (`TessellateTriDomain`, `tessellator.cpp:1015-1066`).
    pub fn triangle(&mut self, outside: [f32; 3], inside: f32) {
        match self.triangle_factors(outside, inside) {
            Processed::Culled => self.clear(),
            Processed::Minimum => {
                self.define_point(0, ONE, 0);
                self.define_point(0, 0, 1);
                self.define_point(ONE, 0, 2);
                self.point_count = 3;
                self.minimum_connectivity(&[[0, 1, 2]]);
            }
            Processed::Full(factors) => {
                self.triangle_points(&factors);
                if self.whole_points_output() {
                    return;
                }
                self.triangle_connectivity(&factors);
            }
        }
    }

    /// A quad patch: the factors of the edges where `u` and `v` are zero and where they are one,
    /// in the order `u=0, v=0, u=1, v=1`, and the inside factors along `u` and `v`
    /// (`TessellateQuadDomain`, `tessellator.cpp:494-547`).
    pub fn quad(&mut self, outside: [f32; 4], inside: [f32; 2]) {
        match self.quad_factors(outside, inside) {
            Processed::Culled => self.clear(),
            Processed::Minimum => {
                self.define_point(0, 0, 0);
                self.define_point(ONE, 0, 1);
                self.define_point(ONE, ONE, 2);
                self.define_point(0, ONE, 3);
                self.point_count = 4;
                self.minimum_connectivity(&[[0, 1, 3], [1, 2, 3]]);
            }
            Processed::Full(factors) => {
                self.quad_points(&factors);
                if self.whole_points_output() {
                    return;
                }
                self.quad_connectivity(&factors);
            }
        }
    }

    /// A patch with every factor 1, as points, lines or its clockwise triangles
    /// (`tessellator.cpp:514-529`, `:1034-1048`).
    fn minimum_connectivity(&mut self, triangles: &[[i32; 3]]) {
        match self.output {
            OutputTopology::TriangleCw | OutputTopology::TriangleCcw => {
                for (n, triangle) in triangles.iter().enumerate() {
                    self.clockwise_triangle(triangle[0], triangle[1], triangle[2], 3 * n as i32);
                }
                self.index_count = 3 * triangles.len() as i32;
            }
            OutputTopology::Point => self.all_points(),
            OutputTopology::Line => self.all_points_as_lines(),
        }
    }

    /// Point and line output index the points in order, with no stitching
    /// (`tessellator.cpp:535-544`); whether it did.
    fn whole_points_output(&mut self) -> bool {
        match self.output {
            OutputTopology::Point => self.all_points(),
            OutputTopology::Line => self.all_points_as_lines(),
            OutputTopology::TriangleCw | OutputTopology::TriangleCcw => return false,
        }
        true
    }

    /// `QuadProcessTessFactors`, `tessellator.cpp:552-729`.
    fn quad_factors(&mut self, outside: [f32; 4], inside: [f32; 2]) -> Processed<QuadFactors> {
        if outside
            .iter()
            .any(|factor| factor.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater))
        {
            return Processed::Culled;
        }
        let (mut lower, upper) = self.bounds();
        let mut outside = outside.map(|factor| fmin(upper, fmax(lower, factor)));
        if self.integer_partitioning() {
            outside = outside.map(f32::ceil);
        }
        // Any factor past 1 forces the inside ones past 1, so the patch has a picture frame.
        if self.partitioning == Partitioning::FractionalOdd
            && outside
                .iter()
                .chain(inside.iter())
                .any(|&factor| factor > MIN_ODD_PLUS_HALF_EPSILON)
        {
            lower = MIN_ODD_FACTOR + EPSILON;
        }
        let mut inside = inside.map(|factor| fmin(upper, fmax(lower, factor)));
        if self.integer_partitioning() {
            inside = inside.map(f32::ceil);
        }
        self.clear();
        let (outside_parity, inside_parity) = if self.integer_partitioning() {
            (
                outside.map(|factor| {
                    if is_even(factor) {
                        Parity::Even
                    } else {
                        Parity::Odd
                    }
                }),
                inside.map(|factor| {
                    if is_even(factor) || is_one(factor) {
                        Parity::Even
                    } else {
                        Parity::Odd
                    }
                }),
            )
        } else {
            ([self.original_parity; 4], [self.original_parity; 2])
        };
        let fixed_outside = outside.map(float_to_fixed);
        let fixed_inside = inside.map(float_to_fixed);
        if (self.integer_partitioning() || self.odd())
            && fixed_inside
                .iter()
                .chain(fixed_outside.iter())
                .all(|&factor| factor == ONE)
        {
            return Processed::Minimum;
        }
        let mut outside_context = [FactorContext::default(); 4];
        for edge in 0..4 {
            self.parity = outside_parity[edge];
            outside_context[edge] = self.factor_context(fixed_outside[edge]);
        }
        let mut inside_context = [FactorContext::default(); 2];
        for axis in 0..2 {
            self.parity = inside_parity[axis];
            inside_context[axis] = self.factor_context(fixed_inside[axis]);
        }
        let mut outside_points = [0; 4];
        for edge in 0..4 {
            self.parity = outside_parity[edge];
            outside_points[edge] = self.points_for_factor(fixed_outside[edge]);
            self.point_count += outside_points[edge];
        }
        self.point_count -= 4;
        let mut inside_points = [0; 2];
        for axis in 0..2 {
            self.parity = inside_parity[axis];
            let least = if inside_parity[axis] == Parity::Odd {
                4
            } else {
                3
            };
            // A floor allows degenerate transition regions when the inside factor is 1.
            inside_points[axis] = self.points_for_factor(fixed_inside[axis]).max(least);
        }
        let inside_base = self.point_count;
        self.point_count += (inside_points[0] - 2) * (inside_points[1] - 2);
        Processed::Full(QuadFactors {
            outside_parity,
            inside_parity,
            outside_context,
            inside_context,
            outside_points,
            inside_points,
            inside_base,
        })
    }

    /// `QuadGeneratePoints`, `tessellator.cpp:734-835`: the outer ring clockwise from the top
    /// left, then each inner ring clockwise from `(0, 1)` spiralling in, then an even patch's
    /// middle row.
    fn quad_points(&mut self, factors: &QuadFactors) {
        let mut offset = 0;
        for edge in 0..4 {
            let end = factors.outside_points[edge] - 1;
            for p in 0..end {
                let q = if edge == 1 || edge == 2 { p } else { end - p };
                self.parity = factors.outside_parity[edge];
                let param = self.place_point(&factors.outside_context[edge], q);
                if edge & 1 == 1 {
                    self.define_point(param, if edge == 3 { ONE } else { 0 }, offset);
                } else {
                    self.define_point(if edge == 2 { ONE } else { 0 }, param, offset);
                }
                offset += 1;
            }
        }
        let [u_points, v_points] = factors.inside_points;
        let rings = u_points.min(v_points) >> 1;
        for ring in 1..rings {
            let start = ring;
            let end = [u_points - 1 - start, v_points - 1 - start];
            for edge in 0..4 {
                let axes = [edge & 1, (edge + 1) & 1];
                let perpendicular = if edge < 2 { start } else { end[axes[0]] };
                self.parity = factors.inside_parity[axes[0]];
                let perpendicular =
                    self.place_point(&factors.inside_context[axes[0]], perpendicular);
                self.parity = factors.inside_parity[axes[1]];
                for p in start..end[axes[1]] {
                    let q = if edge == 1 || edge == 2 {
                        p
                    } else {
                        end[axes[1]] - (p - start)
                    };
                    let param = self.place_point(&factors.inside_context[axes[1]], q);
                    if axes[1] == 1 {
                        self.define_point(perpendicular, param, offset);
                    } else {
                        self.define_point(param, perpendicular, offset);
                    }
                    offset += 1;
                }
            }
        }
        // An even tessellation's innermost ring is a row of points.
        if u_points > v_points && factors.inside_parity[1] == Parity::Even {
            let start = rings;
            let end = u_points - 1 - start;
            self.parity = factors.inside_parity[0];
            for p in start..=end {
                let param = self.place_point(&factors.inside_context[0], p);
                self.define_point(param, ONE_HALF, offset);
                offset += 1;
            }
        } else if v_points >= u_points && factors.inside_parity[0] == Parity::Even {
            let start = rings;
            let end = v_points - 1 - start;
            self.parity = factors.inside_parity[1];
            for p in (start..=end).rev() {
                let param = self.place_point(&factors.inside_context[1], p);
                self.define_point(ONE_HALF, param, offset);
                offset += 1;
            }
        }
    }

    /// `QuadGenerateConnectivity`, `tessellator.cpp:839-1009`.
    fn quad_connectivity(&mut self, factors: &QuadFactors) {
        let [u_points, v_points] = factors.inside_points;
        let rows_to_centre = [(u_points + 1) >> 1, (v_points + 1) >> 1];
        let rings = rows_to_centre[0].min(rows_to_centre[1]);
        // An even partitioning's degenerate row of points breaks the ordering conventions.
        let degenerate_ring = [
            if factors.inside_parity[1] == Parity::Even {
                rows_to_centre[1] - 1
            } else {
                -1
            },
            if factors.inside_parity[0] == Parity::Even {
                rows_to_centre[0] - 1
            } else {
                -1
            },
        ];
        let mut outside_half_points = factors.outside_context.map(|context| context.half_points);
        let mut outside_parity = factors.outside_parity;
        let mut outside_points = factors.outside_points;
        let mut inside_base = factors.inside_base;
        let mut outside_base = 0;
        for ring in 1..rings {
            let inside_points = [u_points - 2 * ring, v_points - 2 * ring];
            let edge0_inside_base = inside_base;
            let edge0_outside_base = outside_base;
            for edge in 0..4 {
                let axis = (edge + 1) & 1;
                let triangles = inside_points[axis] + outside_points[edge] - 2;
                let (inside_offset, outside_offset) = self.quad_edge_offsets(
                    edge,
                    ring == degenerate_ring[axis],
                    (inside_base, outside_base),
                    (edge0_inside_base, edge0_outside_base),
                    (inside_points[axis], outside_points[edge]),
                );
                if ring == 1 {
                    self.stitch_transition(
                        self.index_count,
                        (
                            inside_offset,
                            factors.inside_context[axis].half_points,
                            factors.inside_parity[axis],
                        ),
                        (
                            outside_offset,
                            outside_half_points[edge],
                            outside_parity[edge],
                        ),
                    );
                } else {
                    self.stitch_regular(
                        true,
                        Diagonals::Mirrored,
                        self.index_count,
                        inside_points[axis],
                        inside_offset,
                        outside_offset,
                    );
                }
                self.patched = false;
                self.patched2 = false;
                self.index_count += triangles * 3;
                outside_base += outside_points[edge] - 1;
                if edge == 2 && ring == degenerate_ring[axis] {
                    inside_base -= inside_points[axis] - 1;
                } else {
                    inside_base += inside_points[axis] - 1;
                }
                outside_points[edge] = inside_points[axis];
            }
            if ring == 1 {
                for edge in 0..4 {
                    outside_half_points[edge] = factors.inside_context[edge & 1].half_points;
                    outside_parity[edge] = factors.inside_parity[edge & 1];
                }
            }
        }
        self.quad_centre(factors, outside_base);
    }

    /// Where a quad ring's side starts stitching, its inside row then its outside row, setting
    /// the index rewrite its last side, or a degenerate ring's third, needs
    /// (`tessellator.cpp:883-931`). The bases, the first side's bases and the point counts are each
    /// inside then outside.
    fn quad_edge_offsets(
        &mut self,
        edge: usize,
        degenerate: bool,
        (inside_base, outside_base): (i32, i32),
        (edge0_inside_base, edge0_outside_base): (i32, i32),
        (inside_points, outside_points): (i32, i32),
    ) -> (i32, i32) {
        if edge == 3 && degenerate {
            self.patch2 = IndexPatch2 {
                base_to_invert: inside_base + 1,
                corner_bad: outside_base + outside_points - 1,
                corner_replacement: edge0_outside_base,
                inversion_end: ((inside_base + 1) << 1) - 1,
            };
            self.patched2 = true;
            (self.patch2.base_to_invert, outside_base)
        } else if edge == 3 {
            let inside_bad = inside_points - 1;
            let patch_base = inside_bad + 1;
            self.patch = IndexPatch {
                inside_delta: inside_base,
                inside_bad,
                inside_replacement: edge0_inside_base,
                outside_base: patch_base,
                outside_delta: outside_base - patch_base,
                outside_bad: patch_base + outside_points - 1,
                outside_replacement: edge0_outside_base,
            };
            self.patched = true;
            (0, patch_base)
        } else if edge == 2 && degenerate {
            self.patch2 = IndexPatch2 {
                base_to_invert: inside_base,
                corner_bad: -1,
                corner_replacement: -1,
                inversion_end: inside_base << 1,
            };
            self.patched2 = true;
            (self.patch2.base_to_invert, outside_base)
        } else {
            (inside_base, outside_base)
        }
    }

    /// The centre of a quad: an odd tessellation's leftover row of quads, stitched after the
    /// rings from `outside_base` (`tessellator.cpp:969-1008`).
    fn quad_centre(&mut self, factors: &QuadFactors, outside_base: i32) {
        let [u_points, v_points] = factors.inside_points;
        if u_points > v_points && factors.inside_parity[1] == Parity::Odd {
            self.patched2 = true;
            let quads = (((u_points >> 1) - (v_points >> 1)) << 1)
                + if factors.inside_parity[0] == Parity::Even {
                    2
                } else {
                    1
                };
            let base = outside_base + quads + 2;
            self.patch2 = IndexPatch2 {
                base_to_invert: base,
                corner_bad: base,
                corner_replacement: outside_base,
                inversion_end: base + base + quads,
            };
            self.stitch_regular(
                false,
                Diagonals::InsideToOutside,
                self.index_count,
                quads + 1,
                base,
                outside_base + 1,
            );
            self.patched2 = false;
            self.index_count += quads * 6;
        } else if v_points >= u_points && factors.inside_parity[0] == Parity::Odd {
            self.patched2 = true;
            let quads = (((v_points >> 1) - (u_points >> 1)) << 1)
                + if factors.inside_parity[1] == Parity::Even {
                    2
                } else {
                    1
                };
            let base = outside_base + quads + 1;
            // The reference leaves the corner replacement as the previous rewrite set it; with
            // its bad value -1 no index reaches it.
            self.patch2.base_to_invert = base;
            self.patch2.corner_bad = -1;
            self.patch2.inversion_end = base + base + quads;
            let diagonals = if factors.inside_parity[1] == Parity::Even {
                Diagonals::InsideToOutside
            } else {
                Diagonals::InsideToOutsideExceptMiddle
            };
            self.stitch_regular(
                false,
                diagonals,
                self.index_count,
                quads + 1,
                base,
                outside_base,
            );
            self.patched2 = false;
            self.index_count += quads * 6;
        }
    }

    /// `TriProcessTessFactors`, `tessellator.cpp:1071-1235`.
    fn triangle_factors(&mut self, outside: [f32; 3], inside: f32) -> Processed<TriangleFactors> {
        if outside
            .iter()
            .any(|factor| factor.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater))
        {
            return Processed::Culled;
        }
        let (mut lower, upper) = self.bounds();
        let mut outside = outside.map(|factor| fmin(upper, fmax(lower, factor)));
        if self.integer_partitioning() {
            outside = outside.map(f32::ceil);
        }
        // Only the edge factors force a picture frame: a triangle has one inside factor.
        if self.partitioning == Partitioning::FractionalOdd
            && outside
                .iter()
                .any(|&factor| factor > MIN_ODD_PLUS_HALF_EPSILON)
        {
            lower = MIN_ODD_FACTOR + EPSILON;
        }
        let mut inside = fmin(upper, fmax(lower, inside));
        if self.integer_partitioning() {
            inside = inside.ceil();
        }
        self.clear();
        let (outside_parity, inside_parity) = if self.integer_partitioning() {
            (
                outside.map(|factor| {
                    if is_even(factor) {
                        Parity::Even
                    } else {
                        Parity::Odd
                    }
                }),
                if is_even(inside) || is_one(inside) {
                    Parity::Even
                } else {
                    Parity::Odd
                },
            )
        } else {
            ([self.original_parity; 3], self.original_parity)
        };
        let fixed_outside = outside.map(float_to_fixed);
        let fixed_inside = float_to_fixed(inside);
        if (self.integer_partitioning() || self.odd())
            && fixed_inside == ONE
            && fixed_outside.iter().all(|&factor| factor == ONE)
        {
            return Processed::Minimum;
        }
        let mut outside_context = [FactorContext::default(); 3];
        for edge in 0..3 {
            self.parity = outside_parity[edge];
            outside_context[edge] = self.factor_context(fixed_outside[edge]);
        }
        self.parity = inside_parity;
        let inside_context = self.factor_context(fixed_inside);
        let mut outside_points = [0; 3];
        for edge in 0..3 {
            self.parity = outside_parity[edge];
            outside_points[edge] = self.points_for_factor(fixed_outside[edge]);
            self.point_count += outside_points[edge];
        }
        self.point_count -= 3;
        self.parity = inside_parity;
        let least = if self.odd() { 4 } else { 3 };
        let inside_points = self.points_for_factor(fixed_inside).max(least);
        let inside_base = self.point_count;
        let interior_rings = (inside_points >> 1) - 1;
        self.point_count += if self.odd() {
            3 * (interior_rings * (interior_rings + 1) - interior_rings)
        } else {
            3 * (interior_rings * (interior_rings + 1)) + 1
        };
        Processed::Full(TriangleFactors {
            outside_parity,
            inside_parity,
            outside_context,
            inside_context,
            outside_points,
            inside_points,
            inside_base,
        })
    }

    /// `TriGeneratePoints`, `tessellator.cpp:1240-1333`: the outer ring clockwise from `v = 1`,
    /// then each inner ring spiralling in, then an even patch's centre.
    fn triangle_points(&mut self, factors: &TriangleFactors) {
        let mut offset = 0;
        for edge in 0..3 {
            let end = factors.outside_points[edge] - 1;
            for p in 0..end {
                // Edge 0 has v falling, edge 1 u rising, edge 2 u falling.
                let q = if edge & 1 == 1 { p } else { end - p };
                self.parity = factors.outside_parity[edge];
                let param = self.place_point(&factors.outside_context[edge], q);
                if edge == 0 {
                    self.define_point(0, param, offset);
                } else {
                    let v = if edge == 2 {
                        ONE.wrapping_sub(param)
                    } else {
                        0
                    };
                    self.define_point(param, v, offset);
                }
                offset += 1;
            }
        }
        self.parity = factors.inside_parity;
        let rings = factors.inside_points >> 1;
        for ring in 1..rings {
            let start = ring;
            let end = factors.inside_points - 1 - start;
            for edge in 0..3 {
                let perpendicular = self.place_point(&factors.inside_context, start);
                // Scaled into barycentric space and back to 16.16.
                let perpendicular = perpendicular
                    .wrapping_mul(TWO_THIRDS)
                    .wrapping_add(ONE_HALF)
                    >> FRACTION_BITS;
                for p in start..end {
                    let q = if edge & 1 == 1 { p } else { end - (p - start) };
                    let param = self.place_point(&factors.inside_context, q);
                    // The edge-parallel parameters move in at half the rate.
                    let along = param.wrapping_sub(perpendicular.wrapping_add(1) / 2);
                    match edge {
                        0 => self.define_point(perpendicular, along, offset),
                        1 => self.define_point(along, perpendicular, offset),
                        _ => self.define_point(
                            along,
                            ONE.wrapping_sub(along).wrapping_sub(perpendicular),
                            offset,
                        ),
                    }
                    offset += 1;
                }
            }
        }
        if !self.odd() {
            self.define_point(ONE_THIRD, ONE_THIRD, offset);
        }
    }

    /// `TriGenerateConnectivity`, `tessellator.cpp:1337-1424`.
    fn triangle_connectivity(&mut self, factors: &TriangleFactors) {
        let rings = (factors.inside_points + 1) >> 1;
        let mut outside_half_points = factors.outside_context.map(|context| context.half_points);
        let mut outside_parity = factors.outside_parity;
        let mut outside_points = factors.outside_points;
        let mut inside_base = factors.inside_base;
        let mut outside_base = 0;
        for ring in 1..rings {
            let inside_points = factors.inside_points - 2 * ring;
            let edge0_inside_base = inside_base;
            let edge0_outside_base = outside_base;
            for edge in 0..3 {
                let triangles = inside_points + outside_points[edge] - 2;
                let (inside_offset, outside_offset) = if edge == 2 {
                    let inside_bad = inside_points - 1;
                    let patch_base = inside_bad + 1;
                    self.patch = IndexPatch {
                        inside_delta: inside_base,
                        inside_bad,
                        inside_replacement: edge0_inside_base,
                        outside_base: patch_base,
                        outside_delta: outside_base - patch_base,
                        outside_bad: patch_base + outside_points[edge] - 1,
                        outside_replacement: edge0_outside_base,
                    };
                    self.patched = true;
                    (0, patch_base)
                } else {
                    (inside_base, outside_base)
                };
                if ring == 1 {
                    self.stitch_transition(
                        self.index_count,
                        (
                            inside_offset,
                            factors.inside_context.half_points,
                            factors.inside_parity,
                        ),
                        (
                            outside_offset,
                            outside_half_points[edge],
                            outside_parity[edge],
                        ),
                    );
                } else {
                    self.stitch_regular(
                        true,
                        Diagonals::Mirrored,
                        self.index_count,
                        inside_points,
                        inside_offset,
                        outside_offset,
                    );
                }
                if edge == 2 {
                    self.patched = false;
                }
                self.index_count += triangles * 3;
                outside_base += outside_points[edge] - 1;
                inside_base += inside_points - 1;
                outside_points[edge] = inside_points;
            }
            if ring == 1 {
                outside_half_points = [factors.inside_context.half_points; 3];
                outside_parity = [factors.inside_parity; 3];
            }
        }
        // An odd tessellation's centre is one triangle; the parity is still the inside one the
        // points left.
        if self.odd() {
            self.clockwise_triangle(
                outside_base,
                outside_base + 1,
                outside_base + 2,
                self.index_count,
            );
            self.index_count += 3;
        }
    }

    /// `DefinePoint`, `tessellator.cpp:1628-1636`.
    fn define_point(&mut self, u: Fxp, v: Fxp, offset: i32) {
        self.points[offset as usize] = [fixed_to_float(u), fixed_to_float(v)];
    }

    /// `DefineIndex`, `tessellator.cpp:1641-1648`.
    fn define_index(&mut self, index: i32, offset: i32) {
        let index = self.patch_index(index);
        self.indices[offset as usize] = index as u32;
    }

    /// A clockwise triangle, stored clockwise or reversed for a counter-clockwise output
    /// (`DefineClockwiseTriangle`, `tessellator.cpp:1653-1668`).
    fn clockwise_triangle(&mut self, first: i32, second: i32, third: i32, offset: i32) {
        self.define_index(first, offset);
        if self.output == OutputTopology::TriangleCw {
            self.define_index(second, offset + 1);
            self.define_index(third, offset + 2);
        } else {
            self.define_index(third, offset + 1);
            self.define_index(second, offset + 2);
        }
    }

    /// `DumpAllPoints`, `tessellator.cpp:1673-1679`.
    fn all_points(&mut self) {
        for point in 0..self.point_count {
            self.define_index(point, self.index_count);
            self.index_count += 1;
        }
    }

    /// `DumpAllPointsAsInOrderLineList`, `tessellator.cpp:1684-1691`.
    fn all_points_as_lines(&mut self) {
        for point in 1..self.point_count {
            self.define_index(point - 1, self.index_count);
            self.define_index(point, self.index_count + 1);
            self.index_count += 2;
        }
    }

    /// How many points a factor puts on its edge (`NumPointsForTessFactor`, `tessellator.cpp:1731-1743`).
    fn points_for_factor(&self, factor: Fxp) -> i32 {
        let half = factor.wrapping_add(1) / 2;
        if self.odd() {
            (fxp_ceil(ONE_HALF.wrapping_add(half)).wrapping_mul(2) >> FRACTION_BITS) as i32
        } else {
            (fxp_ceil(half).wrapping_mul(2) >> FRACTION_BITS) as i32 + 1
        }
    }

    /// `ComputeTessFactorContext`, `tessellator.cpp:1748-1788`.
    fn factor_context(&self, factor: Fxp) -> FactorContext {
        let mut half = factor.wrapping_add(1) / 2;
        // A factor of 1 halves to one half, and is tessellated as if odd.
        if self.odd() || half == ONE_HALF {
            half = half.wrapping_add(ONE_HALF);
        }
        let floor = fxp_floor(half);
        let ceil = fxp_ceil(half);
        let half_points = (ceil >> FRACTION_BITS) as i32;
        let split_point_on_floor = if ceil == floor {
            // A value that makes the split never taken.
            half_points + 1
        } else if self.odd() {
            if floor == ONE {
                0
            } else {
                (remove_msb((floor >> FRACTION_BITS) as i32 - 1) << 1) + 1
            }
        } else {
            (remove_msb((floor >> FRACTION_BITS) as i32) << 1) + 1
        };
        let mut floor_segments = (floor.wrapping_mul(2) >> FRACTION_BITS) as usize;
        let mut ceil_segments = (ceil.wrapping_mul(2) >> FRACTION_BITS) as usize;
        if self.odd() {
            floor_segments -= 1;
            ceil_segments -= 1;
        }
        FactorContext {
            inverse_floor_segments: RECIPROCAL[floor_segments],
            inverse_ceil_segments: RECIPROCAL[ceil_segments],
            half_fraction: half.wrapping_sub(floor),
            half_points,
            split_point_on_floor,
        }
    }

    /// Where along its edge, in 16.16, point `point` of a factor lands (`PlacePointIn1D`,
    /// `tessellator.cpp:1793-1845`).
    fn place_point(&self, context: &FactorContext, point: i32) -> Fxp {
        let mut point = point;
        let flip = point >= context.half_points;
        if flip {
            point = (context.half_points << 1) - point;
            if self.odd() {
                point -= 1;
            }
        }
        // The middle is special-cased: the 16-bit arithmetic cannot reach one half exactly.
        if point == context.half_points {
            return ONE_HALF;
        }
        let on_ceil = point as u32;
        let on_floor = if point > context.split_point_on_floor {
            on_ceil.wrapping_sub(1)
        } else {
            on_ceil
        };
        let floor_location = on_floor.wrapping_mul(context.inverse_floor_segments);
        let ceil_location = on_ceil.wrapping_mul(context.inverse_ceil_segments);
        let location = floor_location
            .wrapping_mul(ONE.wrapping_sub(context.half_fraction))
            .wrapping_add(ceil_location.wrapping_mul(context.half_fraction));
        let location = location.wrapping_add(ONE_HALF) >> FRACTION_BITS;
        if flip {
            ONE.wrapping_sub(location)
        } else {
            location
        }
    }

    /// Two parallel rows of points with the same count stitched into triangles
    /// (`StitchRegular`, `tessellator.cpp:1850-1932`).
    fn stitch_regular(
        &mut self,
        trapezoid: bool,
        diagonals: Diagonals,
        base: i32,
        inside_points: i32,
        inside_base: i32,
        outside_base: i32,
    ) {
        let mut base = base;
        let mut inside = inside_base;
        let mut outside = outside_base;
        if trapezoid {
            self.clockwise_triangle(outside, outside + 1, inside, base);
            base += 3;
            outside += 1;
        }
        let mut p;
        match diagonals {
            Diagonals::InsideToOutside => {
                for _ in 0..inside_points - 1 {
                    self.clockwise_triangle(inside, outside, outside + 1, base);
                    self.clockwise_triangle(inside, outside + 1, inside + 1, base + 3);
                    base += 6;
                    inside += 1;
                    outside += 1;
                }
            }
            Diagonals::InsideToOutsideExceptMiddle => {
                p = 0;
                while p < inside_points / 2 - 1 {
                    self.clockwise_triangle(outside, outside + 1, inside, base);
                    self.clockwise_triangle(inside, outside + 1, inside + 1, base + 3);
                    base += 6;
                    inside += 1;
                    outside += 1;
                    p += 1;
                }
                self.clockwise_triangle(outside, inside + 1, inside, base);
                self.clockwise_triangle(outside, outside + 1, inside + 1, base + 3);
                base += 6;
                inside += 1;
                outside += 1;
                p += 2;
                while p < inside_points {
                    self.clockwise_triangle(outside, outside + 1, inside, base);
                    self.clockwise_triangle(inside, outside + 1, inside + 1, base + 3);
                    base += 6;
                    inside += 1;
                    outside += 1;
                    p += 1;
                }
            }
            Diagonals::Mirrored => {
                p = 0;
                while p < inside_points / 2 {
                    self.clockwise_triangle(outside, inside + 1, inside, base);
                    self.clockwise_triangle(outside, outside + 1, inside + 1, base + 3);
                    base += 6;
                    inside += 1;
                    outside += 1;
                    p += 1;
                }
                while p < inside_points - 1 {
                    self.clockwise_triangle(inside, outside, outside + 1, base);
                    self.clockwise_triangle(inside, outside + 1, inside + 1, base + 3);
                    base += 6;
                    inside += 1;
                    outside += 1;
                    p += 1;
                }
            }
        }
        if trapezoid {
            self.clockwise_triangle(outside, outside + 1, inside, base);
        }
    }

    /// Two rows of points with different factors stitched in ruler-function split order
    /// (`StitchTransition`, `tessellator.cpp:1937-2058`). Each row is its first point, its
    /// factor's half-point count and its parity.
    fn stitch_transition(
        &mut self,
        base: i32,
        (inside_base, inside_half, inside_parity): (i32, i32, Parity),
        (outside_base, outside_half, outside_parity): (i32, i32, Parity),
    ) {
        // Where vertex i ends up on the half-edge at the largest factor, in split order.
        const FINAL_POSITION: [i32; 33] = [
            0, 32, 16, 8, 17, 4, 18, 9, 19, 2, 20, 10, 21, 5, 22, 11, 23, 1, 24, 12, 25, 6, 26, 13,
            27, 3, 28, 14, 29, 7, 30, 15, 31,
        ];
        // The first and last entries of the table below a half factor, as loop bounds.
        const LOOP_START: [i32; 33] = [
            1, 1, 17, 9, 9, 5, 5, 5, 5, 3, 3, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            2, 2, 2, 2,
        ];
        const LOOP_END: [i32; 33] = [
            0, 0, 17, 17, 25, 25, 25, 25, 29, 29, 29, 29, 29, 29, 29, 29, 31, 31, 31, 31, 31, 31,
            31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 32,
        ];
        let inside_half = if inside_parity == Parity::Odd {
            inside_half - 1
        } else {
            inside_half
        };
        let outside_half = if outside_parity == Parity::Odd {
            outside_half - 1
        } else {
            outside_half
        };
        let mut base = base;
        let mut outside = outside_base;
        let mut inside = inside_base;
        let start = LOOP_START[inside_half as usize].min(LOOP_START[outside_half as usize]);
        let end = LOOP_END[inside_half as usize].max(LOOP_END[outside_half as usize]);
        if FINAL_POSITION[0] < outside_half {
            self.clockwise_triangle(outside, outside + 1, inside, base);
            base += 3;
            outside += 1;
        }
        for i in start..=end {
            if FINAL_POSITION[i as usize] < inside_half {
                self.clockwise_triangle(inside, outside, inside + 1, base);
                base += 3;
                inside += 1;
            }
            if FINAL_POSITION[i as usize] < outside_half {
                self.clockwise_triangle(outside, outside + 1, inside, base);
                base += 3;
                outside += 1;
            }
        }
        if inside_parity != outside_parity || inside_parity == Parity::Odd {
            if inside_parity == outside_parity {
                // A quad in the middle.
                self.clockwise_triangle(inside, outside, inside + 1, base);
                self.clockwise_triangle(inside + 1, outside, outside + 1, base + 3);
                base += 6;
                inside += 1;
                outside += 1;
            } else if inside_parity == Parity::Even {
                // A triangle pointing in.
                self.clockwise_triangle(inside, outside, outside + 1, base);
                base += 3;
                outside += 1;
            } else {
                // A triangle pointing out.
                self.clockwise_triangle(inside, outside, inside + 1, base);
                base += 3;
                inside += 1;
            }
        }
        for i in (start..=end).rev() {
            if FINAL_POSITION[i as usize] < outside_half {
                self.clockwise_triangle(outside, outside + 1, inside, base);
                base += 3;
                outside += 1;
            }
            if FINAL_POSITION[i as usize] < inside_half {
                self.clockwise_triangle(inside, outside, inside + 1, base);
                base += 3;
                inside += 1;
            }
        }
        if FINAL_POSITION[0] < outside_half {
            self.clockwise_triangle(outside, outside + 1, inside, base);
        }
    }

    /// An index rewritten where stitching wrapped around a ring (`PatchIndexValue`,
    /// `tessellator.cpp:2063-2101`).
    fn patch_index(&self, index: i32) -> i32 {
        if self.patched {
            let patch = &self.patch;
            if index >= patch.outside_base {
                if index == patch.outside_bad {
                    patch.outside_replacement
                } else {
                    index + patch.outside_delta
                }
            } else if index == patch.inside_bad {
                patch.inside_replacement
            } else {
                index + patch.inside_delta
            }
        } else if self.patched2 {
            let patch = &self.patch2;
            if index >= patch.base_to_invert {
                if index == patch.corner_bad {
                    patch.corner_replacement
                } else {
                    patch.inversion_end - index
                }
            } else if index == patch.corner_bad {
                patch.corner_replacement
            } else {
                index
            }
        } else {
            index
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OutputTopology, Partitioning, Tessellator, float_to_fixed};

    /// Twice the signed area of a triangle in the `(u, v)` plane.
    fn doubled_area(points: &[[f32; 2]], triangle: &[u32]) -> f64 {
        let [a, b, c] = [0, 1, 2].map(|n| {
            let [u, v] = points[triangle[n] as usize];
            (f64::from(u), f64::from(v))
        });
        (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)
    }

    /// Every triangle turns the same way and together they cover the domain once: `area` is the
    /// domain's (one for a quad, one half for a triangle).
    fn assert_covers(tessellator: &Tessellator, area: f64) {
        let points = tessellator.points();
        let indices = tessellator.indices();
        assert_eq!(indices.len() % 3, 0);
        let areas: Vec<f64> = indices
            .chunks(3)
            .map(|triangle| doubled_area(points, triangle))
            .collect();
        // The reference's clockwise is with v pointing down: a positive area in (u, v) here, as
        // its single quad's (0,0), (1,0), (0,1) is.
        assert!(areas.iter().all(|&doubled| doubled >= -1e-9), "{areas:?}");
        let total: f64 = areas.iter().sum::<f64>() / 2.0;
        assert!((total - area).abs() < 1e-6, "covered {total}, want {area}");
    }

    fn distinct(points: &[[f32; 2]]) -> bool {
        let mut seen: Vec<[u32; 2]> = points.iter().map(|p| p.map(f32::to_bits)).collect();
        seen.sort_unstable();
        seen.windows(2).all(|pair| pair[0] != pair[1])
    }

    #[test]
    fn fixed_point_conversion_rounds_and_clamps() {
        assert_eq!(float_to_fixed(1.0), 0x1_0000);
        assert_eq!(float_to_fixed(1.5), 0x1_8000);
        assert_eq!(float_to_fixed(64.0), 0x40_0000);
        assert_eq!(float_to_fixed(-3.0), 0);
        assert_eq!(float_to_fixed(f32::NAN), 0);
        // 2^-17 is a half of the last kept bit and rounds to even: down.
        assert_eq!(float_to_fixed(1.0 + 2.0f32.powi(-17)), 0x1_0000);
        // Three halves of it round up.
        assert_eq!(float_to_fixed(1.0 + 3.0 * 2.0f32.powi(-17)), 0x1_0002);
    }

    #[test]
    fn a_triangle_with_every_factor_one_is_itself() {
        let mut tessellator = Tessellator::new(Partitioning::Integer, OutputTopology::TriangleCw);
        tessellator.triangle([1.0; 3], 1.0);
        assert_eq!(tessellator.points(), &[[0.0, 1.0], [0.0, 0.0], [1.0, 0.0]]);
        assert_eq!(tessellator.indices(), &[0, 1, 2]);
        let mut tessellator = Tessellator::new(Partitioning::Integer, OutputTopology::TriangleCcw);
        tessellator.triangle([1.0; 3], 1.0);
        assert_eq!(tessellator.indices(), &[0, 2, 1]);
    }

    #[test]
    fn a_quad_with_every_factor_one_is_two_triangles() {
        let mut tessellator = Tessellator::new(Partitioning::Integer, OutputTopology::TriangleCw);
        tessellator.quad([1.0; 4], [1.0; 2]);
        assert_eq!(
            tessellator.points(),
            &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
        );
        assert_eq!(tessellator.indices(), &[0, 1, 3, 1, 2, 3]);
    }

    #[test]
    fn a_zero_or_nan_edge_factor_culls_the_patch() {
        let mut tessellator = Tessellator::new(Partitioning::Integer, OutputTopology::TriangleCw);
        tessellator.quad([4.0; 4], [4.0; 2]);
        assert!(!tessellator.points().is_empty());
        tessellator.quad([4.0, 0.0, 4.0, 4.0], [4.0; 2]);
        assert!(tessellator.points().is_empty() && tessellator.indices().is_empty());
        tessellator.triangle([2.0, f32::NAN, 2.0], 2.0);
        assert!(tessellator.points().is_empty());
        tessellator.isoline(-1.0, 3.0);
        assert!(tessellator.points().is_empty());
    }

    #[test]
    fn a_quad_at_factor_two_is_its_grid_in_ring_order() {
        let mut tessellator = Tessellator::new(Partitioning::Integer, OutputTopology::TriangleCw);
        tessellator.quad([2.0; 4], [2.0; 2]);
        // The outer ring clockwise from (0, 1) - down u = 0, along v = 0, up u = 1, back along
        // v = 1 - then the centre.
        assert_eq!(
            tessellator.points(),
            &[
                [0.0, 1.0],
                [0.0, 0.5],
                [0.0, 0.0],
                [0.5, 0.0],
                [1.0, 0.0],
                [1.0, 0.5],
                [1.0, 1.0],
                [0.5, 1.0],
                [0.5, 0.5],
            ]
        );
        assert_eq!(tessellator.indices().len(), 8 * 3);
        assert_covers(&tessellator, 1.0);
    }

    #[test]
    fn integer_quads_are_uniform_grids() {
        let mut tessellator = Tessellator::new(Partitioning::Integer, OutputTopology::TriangleCw);
        for n in 1..=64u16 {
            let factor = f32::from(n);
            tessellator.quad([factor; 4], [factor; 2]);
            let points = tessellator.points();
            let segments = f64::from(n);
            let n = usize::from(n);
            assert_eq!(points.len(), (n + 1) * (n + 1), "factor {n}");
            assert_eq!(tessellator.indices().len(), 2 * n * n * 3, "factor {n}");
            assert!(distinct(points), "factor {n}");
            // Each coordinate is k/n, placed as k steps of a 16-bit reciprocal from the nearer end:
            // within half a unit of the last place per step, at most thirty-two steps.
            for point in points {
                for coordinate in point {
                    let k = (f64::from(*coordinate) * segments).round();
                    assert!(
                        (f64::from(*coordinate) - k / segments).abs() <= 2.0f64.powi(-11),
                        "factor {n}: {coordinate}"
                    );
                }
            }
            assert_covers(&tessellator, 1.0);
        }
    }

    #[test]
    fn mixed_triangle_factors_cover_the_domain_once() {
        for partitioning in [
            Partitioning::Integer,
            Partitioning::Pow2,
            Partitioning::FractionalOdd,
            Partitioning::FractionalEven,
        ] {
            let mut tessellator = Tessellator::new(partitioning, OutputTopology::TriangleCw);
            for outside in [[1.0, 2.0, 3.0], [7.5, 2.25, 13.0], [64.0, 1.0, 33.3]] {
                for inside in [1.0, 2.5, 6.0, 40.0] {
                    // Points may repeat: a fractional inside factor near one collapses its ring.
                    tessellator.triangle(outside, inside);
                    assert_covers(&tessellator, 0.5);
                }
            }
        }
    }

    #[test]
    fn mixed_quad_factors_cover_the_domain_once() {
        for partitioning in [
            Partitioning::Integer,
            Partitioning::FractionalOdd,
            Partitioning::FractionalEven,
        ] {
            let mut tessellator = Tessellator::new(partitioning, OutputTopology::TriangleCw);
            for outside in [
                [1.0, 2.0, 3.0, 4.0],
                [9.5, 2.25, 13.0, 5.0],
                [64.0, 1.0, 33.3, 2.0],
            ] {
                for inside in [[1.0, 1.0], [2.5, 7.0], [6.0, 3.0], [40.0, 41.0]] {
                    tessellator.quad(outside, inside);
                    assert_covers(&tessellator, 1.0);
                }
            }
        }
    }

    /// The factors [`sweep`] draws from: the bounds, one either side of each, halves, and values
    /// with fractions that do not land on a sixteenth.
    const SWEEP_FACTORS: [f32; 24] = [
        0.0, 0.5, 1.0, 1.0001, 1.5, 2.0, 2.25, 2.5, 3.0, 3.3, 4.0, 5.75, 7.0, 7.3, 9.5, 13.0, 16.0,
        31.0, 33.3, 40.0, 63.0, 63.5, 64.0, 100.0,
    ];

    /// Every domain, partitioning and output its domain allows, each over every factor alone and
    /// over 48 mixes drawn by a linear congruential generator, as `(domain, partitioning, output,
    /// factors)`. The factors go to [`Tessellator::isoline`] as `(f[1], f[0])`, to
    /// [`Tessellator::triangle`] as `([f[0], f[1], f[2]], f[3])`, and to [`Tessellator::quad`] as
    /// `([f[0]..f[3]], [f[4], f[5]])`.
    fn sweep() -> Vec<(u8, Partitioning, OutputTopology, [f32; 6])> {
        let mut state: u32 = 928;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state >> 8
        };
        let mut cases = Vec::new();
        for domain in 0..3u8 {
            let outputs: &[OutputTopology] = if domain == 0 {
                &[OutputTopology::Point, OutputTopology::Line]
            } else {
                &[
                    OutputTopology::Point,
                    OutputTopology::TriangleCw,
                    OutputTopology::TriangleCcw,
                ]
            };
            for partitioning in [
                Partitioning::Integer,
                Partitioning::Pow2,
                Partitioning::FractionalOdd,
                Partitioning::FractionalEven,
            ] {
                for &output in outputs {
                    for factor in SWEEP_FACTORS {
                        cases.push((domain, partitioning, output, [factor; 6]));
                    }
                    for _ in 0..48 {
                        let factors = [(); 6].map(|()| {
                            let pick = next();
                            if pick & 1 == 0 {
                                SWEEP_FACTORS[(pick >> 1) as usize % SWEEP_FACTORS.len()]
                            } else {
                                // Up to 65.535 in thousandths.
                                f32::from(((pick >> 1) % 65_536) as u16) / 1000.0
                            }
                        });
                        cases.push((domain, partitioning, output, factors));
                    }
                }
            }
        }
        cases
    }

    /// The sweep's output as text: per case a line of `P` then each point's `u,v` float bits in
    /// hex, and a line of `I` then the indices.
    fn sweep_text() -> String {
        use std::fmt::Write as _;
        let mut text = String::new();
        for (domain, partitioning, output, f) in sweep() {
            let mut tessellator = Tessellator::new(partitioning, output);
            match domain {
                0 => tessellator.isoline(f[1], f[0]),
                1 => tessellator.triangle([f[0], f[1], f[2]], f[3]),
                _ => tessellator.quad([f[0], f[1], f[2], f[3]], [f[4], f[5]]),
            }
            text.push('P');
            for point in tessellator.points() {
                let [u, v] = point.map(f32::to_bits);
                write!(text, " {u:08x},{v:08x}").unwrap();
            }
            text.push_str("\nI");
            for index in tessellator.indices() {
                write!(text, " {index}").unwrap();
            }
            text.push('\n');
        }
        text
    }

    /// FNV-1a, 64 bits.
    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// The sweep's output is the reference's to the bit. The pinned hash is of the text
    /// `CHWTessellator` itself writes for the same cases: Mesa's `tessellator.cpp` compiled with
    /// clang 21 as a harness that reads each case's factor bits and prints in [`sweep_text`]'s
    /// form (D928). The harness is not kept; `sweep_cases` prints the cases it reads.
    #[test]
    fn a_sweep_matches_the_reference_bit_for_bit() {
        let text = sweep_text();
        assert_eq!(text.lines().count(), 2 * sweep().len());
        assert_eq!(fnv1a(text.as_bytes()), REFERENCE_SWEEP_HASH);
    }

    /// FNV-1a of the reference's output over [`sweep`].
    const REFERENCE_SWEEP_HASH: u64 = 0xbf84_3c3a_92f4_15c8;

    /// Writes [`sweep`]'s cases, one a line as the domain, partitioning, output and six factor bit
    /// patterns in hex, to the file `ORBISTOUN_TESSELLATOR_SWEEP` names, for a harness around the
    /// reference to read.
    #[test]
    #[ignore = "writes the sweep's cases for the reference harness"]
    fn sweep_cases() {
        use std::fmt::Write as _;
        let path = std::env::var("ORBISTOUN_TESSELLATOR_SWEEP").expect("a path to write to");
        let mut text = String::new();
        for (domain, partitioning, output, factors) in sweep() {
            let bits = factors.map(|factor| format!("{:08x}", factor.to_bits()));
            writeln!(
                text,
                "{domain} {} {} {}",
                partitioning as u8,
                output as u8,
                bits.join(" ")
            )
            .unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn fractional_odd_edges_are_symmetric_with_two_short_segments() {
        // Factor 2.5 under fractional odd rounds up to three segments: the middle one full,
        // the outer two shrinking toward the ends as the factor falls toward one.
        let mut tessellator =
            Tessellator::new(Partitioning::FractionalOdd, OutputTopology::TriangleCw);
        tessellator.quad([2.5; 4], [2.5; 2]);
        let mut edge: Vec<f32> = tessellator
            .points()
            .iter()
            .filter(|point| point[0] == 0.0)
            .map(|point| point[1])
            .collect();
        edge.sort_by(f32::total_cmp);
        // Exact: the reference places the far half as one minus the near half's points.
        let bits = |values: &[f32]| values.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(
            bits(&edge),
            bits(&[0.0, edge[1], 1.0 - edge[1], 1.0]),
            "{edge:?}"
        );
        // The two end segments are the short ones.
        assert!(edge[1] - edge[0] < edge[2] - edge[1]);
    }

    #[test]
    fn edge_factors_clamp_to_the_partitionings_bounds() {
        let mut tessellator = Tessellator::new(Partitioning::Integer, OutputTopology::TriangleCw);
        tessellator.quad([100.0; 4], [100.0; 2]);
        assert_eq!(tessellator.points().len(), 65 * 65);
    }

    #[test]
    fn an_isoline_patch_is_lines_of_segments() {
        let mut tessellator = Tessellator::new(Partitioning::Integer, OutputTopology::Line);
        tessellator.isoline(2.0, 3.0);
        let third = f32::from_bits(0x3eaa_aa00); // 0x5555 / 65536
        let two_thirds = 1.0 - third;
        assert_eq!(
            tessellator.points(),
            &[
                [0.0, 0.0],
                [third, 0.0],
                [two_thirds, 0.0],
                [1.0, 0.0],
                [0.0, 0.5],
                [third, 0.5],
                [two_thirds, 0.5],
                [1.0, 0.5],
            ]
        );
        assert_eq!(tessellator.indices(), &[0, 1, 1, 2, 2, 3, 4, 5, 5, 6, 6, 7]);
        let mut tessellator = Tessellator::new(Partitioning::Integer, OutputTopology::Point);
        tessellator.isoline(2.0, 3.0);
        assert_eq!(tessellator.indices(), &[0, 1, 2, 3, 4, 5, 6, 7]);
    }
}
