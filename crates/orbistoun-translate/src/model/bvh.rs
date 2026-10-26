//! `image_bvh_intersect_ray` and `image_bvh64_intersect_ray`: one ray against one node of a
//! bounding-volume hierarchy (AMD's published RDNA 2 instruction set, section 8.2.10).
//!
//! The guest's own program walks the hierarchy; the instruction reads one node and tests the ray
//! against it. A box node answers the pointers of the children whose boxes the ray enters,
//! nearest first, and `0xffffffff` for the rest; a triangle node answers a distance and two
//! barycentrics over one common denominator. The test is written once, over [`Arithmetic`], so the
//! SPIR-V the translator emits and the host functions the unit tests run are one algorithm.
//!
//! The image family's opcode is bits 24:18 with an eighth bit at bit 0 (table 100), which these two
//! (230, 231) set; guest programs also set bit 0 on ordinary samples, so the table keys the family by
//! the seven bits, where these are 102 and 103 and no other image instruction of this generation
//! sits (`src/amd/compiler/aco_opcodes.py`, its `MIMG` table).

use orbistoun_shader::{Instruction, Operand};
use orbistoun_spirv::{Id, decoration, op};

use super::{
    IMAGE_A16, IMAGE_D16, IMAGE_NSA, Model, SCALAR_REGISTERS, VECTOR_REGISTERS, address_of,
    read_guarded, running_lanes,
};
use crate::TranslateError;

/// The words a node test reads: a box of floats' four child pointers and four six-float boxes. A
/// box of halves and a triangle are 64 bytes and use the first sixteen; a box of floats is 128, its
/// last four words flags the test does not read (`radv_bvh_box32_node`, `radv_bvh_box16_node`,
/// `radv_bvh_triangle_node`, `src/amd/vulkan/bvh/bvh_defines.h:93-148`).
pub(super) const NODE_WORDS: usize = 28;

/// The byte offsets of the last word a 64-byte node and a box of floats are read to.
const SHORT_LAST_BYTE: u32 = 60;
const LONG_LAST_BYTE: u32 = 108;

/// What a box node answers for a child the ray misses, and an instance or procedural node for every
/// word: the invalid node (`RADV_BVH_INVALID_NODE`, `bvh_defines.h:151`).
pub(super) const NO_CHILD: u32 = u32::MAX;

/// The word of a triangle node holding its triangles' flags, a byte a triangle in the order the
/// pointer's type bits number them. Bits 1:0 and 3:2 of a triangle's byte name which of its three
/// vertices' weights return mode 1 answers first and second; return mode 0 answers the whole word
/// as the triangle's id. A node a guest builds for one triangle holds `9` there - the second and
/// third vertices' weights - as Mesa's node writer sets it (`radv_bvh_triangle_node::id`,
/// `bvh_defines.h:93-101`), and as every triangle node in Grand Theft Auto V's captured hierarchy
/// holds it (D991).
const TRIANGLE_FLAGS_WORD: usize = 15;

/// The vertices of each of a triangle node's four triangles, by the pointer's type bits: five
/// vertices, three words apart from word 0, the second to fourth triangles each sharing an edge
/// with one before it and wound as the first. Assumed for the third and fourth (D992): no guest
/// seen builds them.
const TRIANGLE_VERTICES: [[usize; 3]; 4] = [[0, 1, 2], [1, 3, 2], [2, 3, 4], [2, 4, 0]];

/// A node pointer's low three bits are the node's type: 0-3 a triangle, 4 a box of halves, 5 a box
/// of floats, 6 an instance, 7 a procedural box (`bvh_defines.h:12-16`; Mesa's traversal reads bit
/// 2 clear as a triangle, `radv_nir_rt_common.c:1018`).
const NODE_TYPE_MASK: u32 = 7;
const BOX16: u32 = 4;
const BOX32: u32 = 5;
const TRIANGLE_TYPES: u32 = 4;

/// Descriptor fields (RDNA 2 instruction set, table 49): the base address in 256-byte units in bits
/// 39:0, the box growth in bits 62:55, box sorting in bit 63, the last node's index - in 64-byte
/// units - in bits 105:64, the triangle return mode in bit 120 and the descriptor's type in bits
/// 127:124, which a hierarchy's holds as 8.
const BASE_HIGH_MASK: u32 = 0xff;
const GROW_SHIFT: u32 = 23;
const GROW_MASK: u32 = 0xff;
const BOX_SORTING: u32 = 1 << 31;
const LAST_NODE_HIGH_MASK: u32 = 0x3ff;
const BARYCENTRIC_RETURN: u32 = 1 << 24;
const TYPE_SHIFT: u32 = 28;
const HIERARCHY_TYPE: u32 = 8;

/// The instruction's address registers (table 48): the node pointer (two registers for the 64-bit
/// form), the ray's extent, origin, direction and inverse direction - each direction three floats,
/// or under `A16` the two directions' six halves packed into three registers.
const fn address_registers(wide_pointer: bool, a16: bool) -> u32 {
    let pointer = if wide_pointer { 2 } else { 1 };
    let directions = if a16 { 3 } else { 6 };
    pointer + 4 + directions
}

/// The operations the node tests are written in: host arithmetic for the unit tests, SPIR-V for the
/// translator. Every float operation rounds once, as the host's does, and the minimum and maximum
/// ignore a NaN operand.
pub(super) trait Arithmetic {
    /// A single-precision float.
    type Float: Copy;
    /// A 32-bit word.
    type Word: Copy;
    /// A boolean.
    type Truth: Copy;

    /// A float constant.
    fn float(&mut self, value: f32) -> Self::Float;
    /// A word constant.
    fn word(&mut self, value: u32) -> Self::Word;
    /// A word's bits read as a float.
    fn float_of(&mut self, bits: Self::Word) -> Self::Float;
    /// A float's bits as a word.
    fn bits_of(&mut self, value: Self::Float) -> Self::Word;
    /// The half in a word's low (or `high`) sixteen bits, widened.
    fn half_of(&mut self, bits: Self::Word, high: bool) -> Self::Float;
    /// A binary float operation.
    fn float_op(&mut self, operation: FloatOp, lhs: Self::Float, rhs: Self::Float) -> Self::Float;
    /// A float's magnitude.
    fn magnitude(&mut self, value: Self::Float) -> Self::Float;
    /// `lhs < rhs`, false where either is a NaN.
    fn below(&mut self, lhs: Self::Float, rhs: Self::Float) -> Self::Truth;
    /// `lhs >= rhs`, false where either is a NaN.
    fn at_least(&mut self, lhs: Self::Float, rhs: Self::Float) -> Self::Truth;
    /// Whether a float is a NaN.
    fn is_nan(&mut self, value: Self::Float) -> Self::Truth;
    /// Both of two booleans.
    fn and(&mut self, lhs: Self::Truth, rhs: Self::Truth) -> Self::Truth;
    /// Either of two booleans.
    fn or(&mut self, lhs: Self::Truth, rhs: Self::Truth) -> Self::Truth;
    /// A boolean's negation.
    fn not(&mut self, value: Self::Truth) -> Self::Truth;
    /// `condition ? when_true : when_false` on floats.
    fn choose_float(
        &mut self,
        condition: Self::Truth,
        when_true: Self::Float,
        when_false: Self::Float,
    ) -> Self::Float;
    /// `condition ? when_true : when_false` on words.
    fn choose_word(
        &mut self,
        condition: Self::Truth,
        when_true: Self::Word,
        when_false: Self::Word,
    ) -> Self::Word;
    /// A binary word operation, a shift by fewer than thirty-two places.
    fn word_op(&mut self, operation: WordOp, lhs: Self::Word, rhs: Self::Word) -> Self::Word;
    /// `lhs < rhs`, the words read unsigned.
    fn word_below(&mut self, lhs: Self::Word, rhs: Self::Word) -> Self::Truth;
    /// Whether two words are equal.
    fn word_equal(&mut self, lhs: Self::Word, rhs: Self::Word) -> Self::Truth;
}

/// The binary word operations the tests use: a sum wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WordOp {
    And,
    Or,
    Add,
    ShiftLeft,
    ShiftRight,
}

/// The binary float operations the tests use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FloatOp {
    Add,
    Sub,
    Mul,
    /// The lesser, ignoring a NaN operand.
    Min,
    /// The greater, ignoring a NaN operand.
    Max,
}

/// The ray the instruction's address registers carry.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ray<F> {
    /// How far along the ray a hit may lie, in units of the direction.
    pub(super) extent: F,
    pub(super) origin: [F; 3],
    pub(super) direction: [F; 3],
    /// The direction's reciprocal, which the guest supplies rather than the test deriving it.
    pub(super) inverse: [F; 3],
}

/// Which test applies, from the node pointer's type bits.
#[derive(Debug, Clone, Copy)]
pub(super) struct Kind<T> {
    pub(super) triangle: T,
    /// Which triangle of a triangle node: the type bits 1, 2 and 3 ([`TRIANGLE_VERTICES`]); 0 the
    /// first.
    pub(super) which: [T; 3],
    pub(super) box16: T,
    pub(super) box32: T,
}

/// The descriptor's answer options.
#[derive(Debug, Clone, Copy)]
pub(super) struct Options<T, W> {
    /// Box hits nearest first, rather than in the node's order.
    pub(super) sorted: T,
    /// A triangle answers barycentrics (return mode 1) rather than its id and a hit flag.
    pub(super) barycentrics: T,
    /// How many float steps each box's exit distance is pushed out by.
    pub(super) grow: W,
}

/// What picks a node's test and shapes its answer: the node's kind, the descriptor's options, and
/// whether the descriptor holds the node ([`in_range`]).
pub(super) type Selectors<T, W> = (Kind<T>, Options<T, W>, T);

/// The four result words for one node: the box test's for a box, the triangle test's for a
/// triangle, and [`NO_CHILD`] in every word for an instance or a procedural box, whose result Mesa's
/// traversal never reads (`radv_nir_rt_common.c:1018-1092`; assumed), and for a node `valid` says
/// the descriptor does not hold ([`in_range`]).
pub(super) fn intersect<A: Arithmetic>(
    arithmetic: &mut A,
    node: &[A::Word; NODE_WORDS],
    ray: &Ray<A::Float>,
    (kind, options, valid): Selectors<A::Truth, A::Word>,
) -> [A::Word; 4] {
    let wide = box_test(arithmetic, node, ray, false, (options.sorted, options.grow));
    let narrow = box_test(arithmetic, node, ray, true, (options.sorted, options.grow));
    let triangle = triangle_test(arithmetic, node, ray, (options.barycentrics, kind.which));
    let unanswered = arithmetic.word(NO_CHILD);
    let mut result = [unanswered; 4];
    for (slot, ((wide, narrow), triangle)) in result
        .iter_mut()
        .zip(wide.into_iter().zip(narrow).zip(triangle))
    {
        let leaf = arithmetic.choose_word(kind.triangle, triangle, unanswered);
        let half = arithmetic.choose_word(kind.box16, narrow, leaf);
        let answer = arithmetic.choose_word(kind.box32, wide, half);
        *slot = arithmetic.choose_word(valid, answer, unanswered);
    }
    result
}

/// Whether the descriptor holds the node `pointer` names (its low and high words; the high zero for
/// the 32-bit form): a hierarchy's descriptor (type 8), and the node's last 64-byte slot - its
/// second for a box of floats, `long` - no further than the descriptor's last node index. A node
/// it does not hold answers no child in every word (assumed: the instruction set names the fields,
/// not what a node past them answers).
pub(super) fn in_range<A: Arithmetic>(
    arithmetic: &mut A,
    descriptor: [A::Word; 4],
    (low, high): (A::Word, A::Word),
    long: A::Truth,
) -> A::Truth {
    let a = arithmetic;
    let (one, zero) = (a.word(1), a.word(0));
    let (three, twenty_nine) = (a.word(3), a.word(29));
    // The pointer's 64-byte index: the pointer less its three type bits.
    let index_low = a.word_op(WordOp::ShiftRight, low, three);
    let carried = a.word_op(WordOp::ShiftLeft, high, twenty_nine);
    let index_low = a.word_op(WordOp::Or, index_low, carried);
    let index_high = a.word_op(WordOp::ShiftRight, high, three);
    let step = a.choose_word(long, one, zero);
    let last_low = a.word_op(WordOp::Add, index_low, step);
    let wrapped = a.word_below(last_low, index_low);
    let carry = a.choose_word(wrapped, one, zero);
    let last_high = a.word_op(WordOp::Add, index_high, carry);
    let mask = a.word(LAST_NODE_HIGH_MASK);
    let bound_high = a.word_op(WordOp::And, descriptor[3], mask);
    let below_high = a.word_below(last_high, bound_high);
    let same_high = a.word_equal(last_high, bound_high);
    let past_low = a.word_below(descriptor[2], last_low);
    let within_low = a.not(past_low);
    let within_low = a.and(same_high, within_low);
    let within = a.or(below_high, within_low);
    let shift = a.word(TYPE_SHIFT);
    let kind = a.word_op(WordOp::ShiftRight, descriptor[3], shift);
    let hierarchy = a.word(HIERARCHY_TYPE);
    let hierarchy = a.word_equal(kind, hierarchy);
    a.and(within, hierarchy)
}

/// Bound `axis` (0-2 the minimum's x, y, z, 3-5 the maximum's) of child `child`'s box: a float at
/// word 4 + 6 * child + axis, or for a box of halves the half at that index past word 4, two to a
/// word with the first low (`radv_aabb16`, `bvh_defines.h:128-140`).
fn bound<A: Arithmetic>(
    arithmetic: &mut A,
    node: &[A::Word; NODE_WORDS],
    half: bool,
    (child, axis): (usize, usize),
) -> A::Float {
    let index = 6 * child + axis;
    if half {
        arithmetic.half_of(node[4 + index / 2], index % 2 == 1)
    } else {
        arithmetic.float_of(node[4 + index])
    }
}

/// The slab test of each of a box node's four children, as Mesa's software path performs it
/// (`radv_nir_rt_common.c:81-180`): the inverse direction clamped to the finite range so a zero
/// direction gives no NaN, a child whose minimum x is a NaN inactive, and a hit where the ray leaves
/// the box no earlier than it enters it, not behind the origin, and enters it before the extent.
/// The exit is first pushed out by the descriptor's growth, that many float steps (D992). Hits are
/// ordered by where the ray enters, from its origin for a box it starts inside, with a
/// five-exchange sorting network that keeps the node's order between equal distances.
fn box_test<A: Arithmetic>(
    arithmetic: &mut A,
    node: &[A::Word; NODE_WORDS],
    ray: &Ray<A::Float>,
    half: bool,
    (sorted, grow): (A::Truth, A::Word),
) -> [A::Word; 4] {
    let (largest, smallest) = (arithmetic.float(f32::MAX), arithmetic.float(-f32::MAX));
    let (zero, infinity) = (arithmetic.float(0.0), arithmetic.float(f32::INFINITY));
    let missed = arithmetic.word(NO_CHILD);
    let mut inverse = ray.inverse;
    for component in &mut inverse {
        let capped = arithmetic.float_op(FloatOp::Min, *component, largest);
        *component = arithmetic.float_op(FloatOp::Max, capped, smallest);
    }

    let mut distances = [infinity; 4];
    let mut children = [missed; 4];
    for child in 0..4 {
        let mut near = None;
        let mut far = None;
        for (axis, (&origin, &scale)) in ray.origin.iter().zip(&inverse).enumerate() {
            let low = bound(arithmetic, node, half, (child, axis));
            let high = bound(arithmetic, node, half, (child, axis + 3));
            let to_low = arithmetic.float_op(FloatOp::Sub, low, origin);
            let to_low = arithmetic.float_op(FloatOp::Mul, to_low, scale);
            let to_high = arithmetic.float_op(FloatOp::Sub, high, origin);
            let to_high = arithmetic.float_op(FloatOp::Mul, to_high, scale);
            let enter = arithmetic.float_op(FloatOp::Min, to_low, to_high);
            let leave = arithmetic.float_op(FloatOp::Max, to_low, to_high);
            near = Some(match near {
                Some(near) => arithmetic.float_op(FloatOp::Max, near, enter),
                None => enter,
            });
            far = Some(match far {
                Some(far) => arithmetic.float_op(FloatOp::Min, far, leave),
                None => leave,
            });
        }
        let (Some(near), Some(far)) = (near, far) else {
            continue;
        };
        let low_x = bound(arithmetic, node, half, (child, 0));
        let inactive = arithmetic.is_nan(low_x);
        let active = arithmetic.not(inactive);
        // The exit pushed out by the descriptor's growth, in float steps, short of infinity: a
        // non-negative exit's bits order as its values, a negative zero's sign cleared.
        let exit = arithmetic.bits_of(far);
        let magnitude = arithmetic.word(0x7fff_ffff);
        let exit = arithmetic.word_op(WordOp::And, exit, magnitude);
        let grown = arithmetic.word_op(WordOp::Add, exit, grow);
        let unbounded = arithmetic.word(f32::INFINITY.to_bits());
        let short = arithmetic.word_below(grown, unbounded);
        let grown = arithmetic.choose_word(short, grown, unbounded);
        let grown = arithmetic.float_of(grown);
        let ahead = arithmetic.at_least(far, zero);
        let before_exit = arithmetic.at_least(grown, near);
        let ahead = arithmetic.and(ahead, before_exit);
        let within = arithmetic.below(near, ray.extent);
        let entered = arithmetic.and(ahead, within);
        let hit = arithmetic.and(active, entered);
        // Ordered by where the ray enters, from its origin where it starts inside.
        let depth = arithmetic.float_op(FloatOp::Max, near, zero);
        distances[child] = arithmetic.choose_float(hit, depth, infinity);
        children[child] = arithmetic.choose_word(hit, node[child], missed);
    }

    let unsorted = children;
    for (first, second) in [(0, 1), (2, 3), (0, 2), (1, 3), (1, 2)] {
        let exchange = arithmetic.below(distances[second], distances[first]);
        let (near, far) = (distances[first], distances[second]);
        distances[first] = arithmetic.choose_float(exchange, far, near);
        distances[second] = arithmetic.choose_float(exchange, near, far);
        let (near, far) = (children[first], children[second]);
        children[first] = arithmetic.choose_word(exchange, far, near);
        children[second] = arithmetic.choose_word(exchange, near, far);
    }
    let mut result = [missed; 4];
    for (slot, (sorted_child, node_order)) in
        result.iter_mut().zip(children.into_iter().zip(unsorted))
    {
        *slot = arithmetic.choose_word(sorted, sorted_child, node_order);
    }
    result
}

/// `vector` with its components reordered so the third is the one `major` names (`x_major` the
/// first, else `y_major` the second, else the third) and the other two follow it cyclically - then
/// those two exchanged where `flip` holds.
fn reorder<A: Arithmetic>(
    arithmetic: &mut A,
    vector: [A::Float; 3],
    (x_major, y_major, flip): (A::Truth, A::Truth, A::Truth),
) -> [A::Float; 3] {
    let [x, y, z] = vector;
    let pick = |arithmetic: &mut A, if_x: A::Float, if_y: A::Float, otherwise: A::Float| {
        let rest = arithmetic.choose_float(y_major, if_y, otherwise);
        arithmetic.choose_float(x_major, if_x, rest)
    };
    let third = pick(arithmetic, x, y, z);
    let first = pick(arithmetic, y, z, x);
    let second = pick(arithmetic, z, x, y);
    [
        arithmetic.choose_float(flip, second, first),
        arithmetic.choose_float(flip, first, second),
        third,
    ]
}

/// The watertight ray-triangle test (Woop, Benthin and Wald, "Watertight Ray/Triangle
/// Intersection", JCGT 2(1), 2013), which Mesa's software path also performs
/// (`radv_nir_rt_common.c:199-382`): the vertices are moved to the ray's origin, reordered so the
/// direction's largest component is the third, and sheared so the ray runs along it. `u`, `v` and
/// `w`, the scaled barycentrics of the three vertices, hit where none has a sign the others lack,
/// their sum is not zero, and the distance is not behind the origin.
///
/// The triangle is the one of the node's four the type bits name (`which`, [`TRIANGLE_VERTICES`]).
/// Its weights `u`, `v`, `w` are its vertices' in its own order, and `det` their sum. The distance
/// `t` is their sum weighted by each vertex's sheared depth, so `t / det` is the hit's distance in
/// units of the direction. A hit's `t` and `det` share a sign, the triangle's facing: positive where
/// it is wound counter-clockwise as the ray sees it, negative where clockwise (D992). A shader that
/// culls a facing tests that sign; the instruction culls nothing.
///
/// Return mode 1 answers `{t, det, I, J}`: `I` and `J` the weights of the vertices the triangle's
/// flags byte names ([`TRIANGLE_FLAGS_WORD`]), the second and third for the usual `9`, whose
/// quotients by `det` are the hit's barycentrics (Mesa divides them so,
/// `radv_nir_rt_common.c:631-675`). Mode 0 answers `{t, det, flags, hit}`, the flags word whole and
/// one where the ray hits. A miss answers an infinite `t` over a `det` of one, a distance no extent
/// admits (Mesa's software miss, `radv_nir_rt_common.c:226`), with the same last two words, and a
/// hit flag of zero. The extent is not the instruction's to apply to a triangle: the shader
/// compares `t / det` with it. A shared edge or vertex hits both triangles; Mesa's tie-break for it
/// is a Vulkan rule, not the instruction's.
fn triangle_test<A: Arithmetic>(
    arithmetic: &mut A,
    node: &[A::Word; NODE_WORDS],
    ray: &Ray<A::Float>,
    (barycentrics, which): (A::Truth, [A::Truth; 3]),
) -> [A::Word; 4] {
    let zero = arithmetic.float(0.0);
    let [along_x, along_y, along_z] = ray
        .direction
        .map(|component| arithmetic.magnitude(component));
    let x_over_y = arithmetic.at_least(along_x, along_y);
    let x_over_z = arithmetic.at_least(along_x, along_z);
    let y_over_z = arithmetic.at_least(along_y, along_z);
    let x_major = arithmetic.and(x_over_y, x_over_z);
    let y_over_x = arithmetic.not(x_over_y);
    let y_major = arithmetic.and(y_over_x, y_over_z);
    let no_flip = arithmetic.below(zero, zero);
    let direction = reorder(arithmetic, ray.direction, (x_major, y_major, no_flip));
    // The winding is kept by exchanging the other two where the major component is negative.
    let flip = arithmetic.below(direction[2], zero);
    let order = (x_major, y_major, flip);
    let direction = reorder(arithmetic, ray.direction, order);
    let inverse = reorder(arithmetic, ray.inverse, order);
    let scale = inverse[2];
    let shear_x = arithmetic.float_op(FloatOp::Mul, direction[0], scale);
    let shear_y = arithmetic.float_op(FloatOp::Mul, direction[1], scale);

    // A word of the named triangle's vertex: the first triangle's unless a later one is named.
    let vertex_word = |arithmetic: &mut A, vertex: usize, axis: usize| {
        let words = TRIANGLE_VERTICES.map(|triangle| node[3 * triangle[vertex] + axis]);
        let chosen = arithmetic.choose_word(which[0], words[1], words[0]);
        let chosen = arithmetic.choose_word(which[1], words[2], chosen);
        arithmetic.choose_word(which[2], words[3], chosen)
    };

    // Each vertex relative to the origin, sheared: x, y in the ray's plane and z along it.
    let mut sheared = [[zero; 3]; 3];
    for (vertex, place) in sheared.iter_mut().enumerate() {
        let mut relative = [zero; 3];
        for (axis, component) in relative.iter_mut().enumerate() {
            let word = vertex_word(arithmetic, vertex, axis);
            let coordinate = arithmetic.float_of(word);
            *component = arithmetic.float_op(FloatOp::Sub, coordinate, ray.origin[axis]);
        }
        let [x, y, z] = reorder(arithmetic, relative, order);
        let offset_x = arithmetic.float_op(FloatOp::Mul, shear_x, z);
        let offset_y = arithmetic.float_op(FloatOp::Mul, shear_y, z);
        *place = [
            arithmetic.float_op(FloatOp::Sub, x, offset_x),
            arithmetic.float_op(FloatOp::Sub, y, offset_y),
            arithmetic.float_op(FloatOp::Mul, scale, z),
        ];
    }
    let [first, second, third] = sheared;
    let edge = |arithmetic: &mut A, from: [A::Float; 3], to: [A::Float; 3]| {
        let left = arithmetic.float_op(FloatOp::Mul, to[0], from[1]);
        let right = arithmetic.float_op(FloatOp::Mul, to[1], from[0]);
        arithmetic.float_op(FloatOp::Sub, left, right)
    };
    let weight_first = edge(arithmetic, second, third);
    let weight_second = edge(arithmetic, third, first);
    let weight_third = edge(arithmetic, first, second);
    let weights = [weight_first, weight_second, weight_third];

    let mut negative = arithmetic.below(zero, zero);
    let mut positive = negative;
    for weight in weights {
        let below = arithmetic.below(weight, zero);
        negative = arithmetic.or(negative, below);
        let above = arithmetic.below(zero, weight);
        positive = arithmetic.or(positive, above);
    }
    let straddles = arithmetic.and(negative, positive);
    let inside = arithmetic.not(straddles);

    let rest = arithmetic.float_op(FloatOp::Add, weight_second, weight_third);
    let determinant = arithmetic.float_op(FloatOp::Add, weight_first, rest);
    let mut distance = zero;
    for (index, (weight, vertex)) in weights.into_iter().zip(sheared).enumerate() {
        let term = arithmetic.float_op(FloatOp::Mul, weight, vertex[2]);
        distance = if index == 0 {
            term
        } else {
            arithmetic.float_op(FloatOp::Add, distance, term)
        };
    }
    let facing_back = arithmetic.below(determinant, zero);
    let facing_front = arithmetic.below(zero, determinant);
    let facing = arithmetic.or(facing_back, facing_front);
    let behind_front = arithmetic.below(distance, zero);
    let behind_front = arithmetic.and(facing_front, behind_front);
    let behind_back = arithmetic.below(zero, distance);
    let behind_back = arithmetic.and(facing_back, behind_back);
    let behind = arithmetic.or(behind_front, behind_back);
    let ahead = arithmetic.not(behind);
    let hit = arithmetic.and(inside, facing);
    let hit = arithmetic.and(hit, ahead);

    triangle_answer(
        arithmetic,
        node,
        (hit, distance, determinant, weights),
        (barycentrics, which),
    )
}

/// A triangle test's four words from its result ([`triangle_test`]): `t` and `det`, an infinite
/// `t` over one for a miss, then in return mode 1 the two weights the flags byte of the triangle
/// `which` names, in mode 0 the flags word and the hit flag.
fn triangle_answer<A: Arithmetic>(
    arithmetic: &mut A,
    node: &[A::Word; NODE_WORDS],
    (hit, distance, determinant, weights): (A::Truth, A::Float, A::Float, [A::Float; 3]),
    (barycentrics, which): (A::Truth, [A::Truth; 3]),
) -> [A::Word; 4] {
    let (infinity, one) = (arithmetic.float(f32::INFINITY), arithmetic.float(1.0));
    let distance = arithmetic.choose_float(hit, distance, infinity);
    let distance = arithmetic.bits_of(distance);
    let denominator = arithmetic.choose_float(hit, determinant, one);
    let denominator = arithmetic.bits_of(denominator);

    // The flags byte of the named triangle, and the weights it names: 1 the second vertex's, 2
    // the third's, else the first's.
    let flags = node[TRIANGLE_FLAGS_WORD];
    let (zero_word, eight, sixteen, twenty_four) = (
        arithmetic.word(0),
        arithmetic.word(8),
        arithmetic.word(16),
        arithmetic.word(24),
    );
    let shift = arithmetic.choose_word(which[0], eight, zero_word);
    let shift = arithmetic.choose_word(which[1], sixteen, shift);
    let shift = arithmetic.choose_word(which[2], twenty_four, shift);
    let byte = arithmetic.word_op(WordOp::ShiftRight, flags, shift);
    let weights = weights.map(|weight| arithmetic.bits_of(weight));
    let (two, three) = (arithmetic.word(2), arithmetic.word(3));
    let named = |arithmetic: &mut A, at: A::Word| {
        let index = arithmetic.word_op(WordOp::ShiftRight, byte, at);
        let index = arithmetic.word_op(WordOp::And, index, three);
        let one_word = arithmetic.word(1);
        let second = arithmetic.word_equal(index, one_word);
        let third = arithmetic.word_equal(index, two);
        let chosen = arithmetic.choose_word(second, weights[1], weights[0]);
        arithmetic.choose_word(third, weights[2], chosen)
    };
    let first_named = named(arithmetic, zero_word);
    let second_named = named(arithmetic, two);
    let set = arithmetic.word(1);
    let flag = arithmetic.choose_word(hit, set, zero_word);
    [
        distance,
        denominator,
        arithmetic.choose_word(barycentrics, first_named, flags),
        arithmetic.choose_word(barycentrics, second_named, flag),
    ]
}

/// Host arithmetic, which the unit tests and the host evaluator ([`host_intersect_ray`]) run the
/// node tests in.
#[derive(Debug, Default)]
struct Host;

impl Arithmetic for Host {
    type Float = f32;
    type Word = u32;
    type Truth = bool;

    fn float(&mut self, value: f32) -> f32 {
        value
    }
    fn word(&mut self, value: u32) -> u32 {
        value
    }
    fn float_of(&mut self, bits: u32) -> f32 {
        f32::from_bits(bits)
    }
    fn bits_of(&mut self, value: f32) -> u32 {
        value.to_bits()
    }
    fn half_of(&mut self, bits: u32, high: bool) -> f32 {
        let shifted = if high { bits >> 16 } else { bits };
        half_to_float(u16::try_from(shifted & 0xffff).unwrap_or(0))
    }
    fn float_op(&mut self, operation: FloatOp, lhs: f32, rhs: f32) -> f32 {
        match operation {
            FloatOp::Add => lhs + rhs,
            FloatOp::Sub => lhs - rhs,
            FloatOp::Mul => lhs * rhs,
            FloatOp::Min => lhs.min(rhs),
            FloatOp::Max => lhs.max(rhs),
        }
    }
    fn magnitude(&mut self, value: f32) -> f32 {
        value.abs()
    }
    fn below(&mut self, lhs: f32, rhs: f32) -> bool {
        lhs < rhs
    }
    fn at_least(&mut self, lhs: f32, rhs: f32) -> bool {
        lhs >= rhs
    }
    fn is_nan(&mut self, value: f32) -> bool {
        value.is_nan()
    }
    fn and(&mut self, lhs: bool, rhs: bool) -> bool {
        lhs && rhs
    }
    fn or(&mut self, lhs: bool, rhs: bool) -> bool {
        lhs || rhs
    }
    fn not(&mut self, value: bool) -> bool {
        !value
    }
    fn choose_float(&mut self, condition: bool, when_true: f32, when_false: f32) -> f32 {
        if condition { when_true } else { when_false }
    }
    fn choose_word(&mut self, condition: bool, when_true: u32, when_false: u32) -> u32 {
        if condition { when_true } else { when_false }
    }
    fn word_op(&mut self, operation: WordOp, lhs: u32, rhs: u32) -> u32 {
        match operation {
            WordOp::And => lhs & rhs,
            WordOp::Or => lhs | rhs,
            WordOp::Add => lhs.wrapping_add(rhs),
            WordOp::ShiftLeft => lhs << (rhs & 31),
            WordOp::ShiftRight => lhs >> (rhs & 31),
        }
    }
    fn word_below(&mut self, lhs: u32, rhs: u32) -> bool {
        lhs < rhs
    }
    fn word_equal(&mut self, lhs: u32, rhs: u32) -> bool {
        lhs == rhs
    }
}

/// An IEEE half widened to a single: exponent 31 is infinity or NaN, exponent 0 a subnormal.
fn half_to_float(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let fraction = bits & 0x3ff;
    let mantissa = f32::from(fraction);
    sign * match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 if fraction == 0 => f32::INFINITY,
        31 => f32::NAN,
        _ => (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15),
    }
}

/// The node test's type and descriptor bits on the host, for a pointer's low word and a descriptor.
fn host_selectors(pointer: u32, descriptor: [u32; 4]) -> (Kind<bool>, Options<bool, u32>) {
    let kind = pointer & NODE_TYPE_MASK;
    (
        Kind {
            triangle: kind < TRIANGLE_TYPES,
            which: [kind == 1, kind == 2, kind == 3],
            box16: kind == BOX16,
            box32: kind == BOX32,
        },
        Options {
            sorted: descriptor[1] & BOX_SORTING != 0,
            barycentrics: descriptor[3] & BARYCENTRIC_RETURN != 0,
            grow: descriptor[1] >> GROW_SHIFT & GROW_MASK,
        },
    )
}

/// The guest address of the node `pointer` names: the descriptor's base, in 256-byte units, plus
/// the pointer with its type bits cleared, in 8-byte units (RDNA 2 instruction set, table 49;
/// `build_node_to_addr`, `radv_nir_rt_common.c:394-402`). [`node_address_words`] is its emitted
/// form.
fn node_address(descriptor: [u32; 4], pointer: u64) -> u64 {
    let base = (u64::from(descriptor[1] & BASE_HIGH_MASK) << 32 | u64::from(descriptor[0])) << 8;
    base.wrapping_add((pointer & !u64::from(NODE_TYPE_MASK)) << 3)
}

/// SPIR-V arithmetic through a [`Model`], each float constant declared once per instruction.
struct Emit<'m, M: Model + ?Sized> {
    model: &'m mut M,
    floats: Vec<(u32, Id)>,
}

impl<M: Model + ?Sized> Emit<'_, M> {
    /// One float operation on float-typed values, its result kept from contraction so a driver
    /// rounds each step as the host does.
    fn float_instruction(&mut self, opcode: u16, lhs: Id, rhs: Id) -> Id {
        let f32_type = self.model.f32_type();
        let b = self.model.builder();
        let result = b.id();
        b.function(opcode, &[f32_type.0, result.0, lhs.0, rhs.0]);
        b.annotate(op::DECORATE, &[result.0, decoration::NO_CONTRACTION]);
        result
    }

    /// A `GLSL.std.450` operation on float-typed values.
    fn extended(&mut self, instruction: u32, operands: &[Id]) -> Id {
        let (f32_type, set) = (self.model.f32_type(), self.model.glsl_set());
        self.model
            .builder()
            .ext_inst(f32_type, set, instruction, operands)
    }

    fn truth(&mut self, opcode: u16, operands: &[Id]) -> Id {
        let bool_type = self.model.bool_type();
        let b = self.model.builder();
        let result = b.id();
        let mut words = vec![bool_type.0, result.0];
        words.extend(operands.iter().map(|id| id.0));
        b.function(opcode, &words);
        result
    }
}

/// `GLSL.std.450` instruction numbers: `FAbs`, `NMin`, `NMax` (which answer the other operand for a
/// NaN) and `UnpackHalf2x16`.
const GLSL_FABS: u32 = 4;
const GLSL_NMIN: u32 = 79;
const GLSL_NMAX: u32 = 80;
const GLSL_UNPACK_HALF_2X16: u32 = 62;

impl<M: Model + ?Sized> Arithmetic for Emit<'_, M> {
    type Float = Id;
    type Word = Id;
    type Truth = Id;

    fn float(&mut self, value: f32) -> Id {
        let bits = value.to_bits();
        if let Some(&(_, id)) = self.floats.iter().find(|(known, _)| *known == bits) {
            return id;
        }
        let word = self.model.constant(bits);
        let id = self.model.as_float(word);
        self.floats.push((bits, id));
        id
    }
    fn word(&mut self, value: u32) -> Id {
        self.model.constant(value)
    }
    fn float_of(&mut self, bits: Id) -> Id {
        self.model.as_float(bits)
    }
    fn bits_of(&mut self, value: Id) -> Id {
        let u32_type = self.model.u32_type();
        let b = self.model.builder();
        let result = b.id();
        b.function(op::BITCAST, &[u32_type.0, result.0, value.0]);
        result
    }
    fn half_of(&mut self, bits: Id, high: bool) -> Id {
        let (f32_type, set) = (self.model.f32_type(), self.model.glsl_set());
        let b = self.model.builder();
        let pair_type = b.declare_type(op::TYPE_VECTOR, &[f32_type.0, 2]);
        let pair = b.ext_inst(pair_type, set, GLSL_UNPACK_HALF_2X16, &[bits]);
        let result = b.id();
        b.function(
            op::COMPOSITE_EXTRACT,
            &[f32_type.0, result.0, pair.0, u32::from(high)],
        );
        result
    }
    fn float_op(&mut self, operation: FloatOp, lhs: Id, rhs: Id) -> Id {
        match operation {
            FloatOp::Add => self.float_instruction(op::FADD, lhs, rhs),
            FloatOp::Sub => self.float_instruction(op::FSUB, lhs, rhs),
            FloatOp::Mul => self.float_instruction(op::FMUL, lhs, rhs),
            FloatOp::Min => self.extended(GLSL_NMIN, &[lhs, rhs]),
            FloatOp::Max => self.extended(GLSL_NMAX, &[lhs, rhs]),
        }
    }
    fn magnitude(&mut self, value: Id) -> Id {
        self.extended(GLSL_FABS, &[value])
    }
    fn below(&mut self, lhs: Id, rhs: Id) -> Id {
        self.truth(op::FORD_LESS_THAN, &[lhs, rhs])
    }
    fn at_least(&mut self, lhs: Id, rhs: Id) -> Id {
        self.truth(op::FORD_GREATER_THAN_EQUAL, &[lhs, rhs])
    }
    fn is_nan(&mut self, value: Id) -> Id {
        self.truth(op::IS_NAN, &[value])
    }
    fn and(&mut self, lhs: Id, rhs: Id) -> Id {
        self.model.both(lhs, rhs)
    }
    fn or(&mut self, lhs: Id, rhs: Id) -> Id {
        self.model.either(lhs, rhs)
    }
    fn not(&mut self, value: Id) -> Id {
        self.truth(op::LOGICAL_NOT, &[value])
    }
    fn choose_float(&mut self, condition: Id, when_true: Id, when_false: Id) -> Id {
        let f32_type = self.model.f32_type();
        let b = self.model.builder();
        let result = b.id();
        b.function(
            op::SELECT,
            &[f32_type.0, result.0, condition.0, when_true.0, when_false.0],
        );
        result
    }
    fn choose_word(&mut self, condition: Id, when_true: Id, when_false: Id) -> Id {
        self.model.select(condition, when_true, when_false)
    }
    fn word_op(&mut self, operation: WordOp, lhs: Id, rhs: Id) -> Id {
        let opcode = match operation {
            WordOp::And => op::BITWISE_AND,
            WordOp::Or => op::BITWISE_OR,
            WordOp::Add => op::IADD,
            WordOp::ShiftLeft => op::SHIFT_LEFT_LOGICAL,
            WordOp::ShiftRight => op::SHIFT_RIGHT_LOGICAL,
        };
        self.model.binary(opcode, lhs, rhs)
    }
    fn word_below(&mut self, lhs: Id, rhs: Id) -> Id {
        self.model.compare(op::ULESS_THAN, lhs, rhs)
    }
    fn word_equal(&mut self, lhs: Id, rhs: Id) -> Id {
        self.model.compare(op::IEQUAL, lhs, rhs)
    }
}

/// Whether an instruction is one of the two this module translates.
pub(crate) fn is_intersect_ray(name: &str) -> bool {
    matches!(
        name,
        "image_bvh_intersect_ray" | "image_bvh64_intersect_ray"
    )
}

/// The operands a checked intersection reads.
struct Operands {
    destination: u32,
    addresses: Vec<u32>,
    descriptor: u32,
    wide_pointer: bool,
    a16: bool,
}

/// Checks the instruction's fields and operands: the restrictions the instruction set states (data
/// mask `0xf`, no `D16`, a 128-bit descriptor, no `TFE` or `LWE`) and registers that fit the files.
fn checked_operands(instruction: &Instruction, name: &str) -> Result<Operands, TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let (
        Some(Operand::Vector(destination)),
        Some(Operand::Vector(first)),
        Some(Operand::Scalar(descriptor)),
    ) = (
        instruction.operands.first(),
        instruction.operands.get(1),
        instruction.operands.get(2),
    )
    else {
        return Err(refuse(
            "a ray intersection is not a destination, an address and a descriptor",
        ));
    };
    let second = instruction.second_word.unwrap_or(0);
    // `DMASK` 11:8, `R128` 15, `TFE` 16 and `LWE` 17 of the first word.
    let fields = ((instruction.word >> 8) & 0xf) | (((instruction.word >> 15) & 0x7) << 4);
    if fields != 0x1f || second & IMAGE_D16 != 0 {
        return Err(refuse(concat!(
            "a ray intersection without the fields the instruction set requires: a data mask of ",
            "0xf, a 128-bit descriptor, and no sixteen-bit data or status word"
        )));
    }
    let wide_pointer = name == "image_bvh64_intersect_ray";
    let a16 = second & IMAGE_A16 != 0;
    let used = address_registers(wide_pointer, a16);
    let addresses = address_list(instruction, u32::from(*first), used)
        .ok_or_else(|| refuse("a ray intersection's address registers run past the file"))?;
    let destination = u32::from(*destination);
    if destination + 4 > VECTOR_REGISTERS || u32::from(*descriptor) + 4 > SCALAR_REGISTERS {
        return Err(refuse(
            "a ray intersection's result or descriptor runs past its register file",
        ));
    }
    Ok(Operands {
        destination,
        addresses,
        descriptor: u32::from(*descriptor),
        wide_pointer,
        a16,
    })
}

/// The vector register of each of `used` address registers: consecutive from `first`, or for an
/// `NSA` encoding the rest listed a byte each in the dwords after the fixed part
/// (`aco_assembler.cpp:744-777`). `None` where one is past the file or not in the instruction.
fn address_list(instruction: &Instruction, first: u32, used: u32) -> Option<Vec<u32>> {
    let (shift, mask) = IMAGE_NSA;
    let nsa_dwords = (instruction.word >> shift) & mask;
    let mut addresses = vec![first];
    for index in 1..used {
        let register = if nsa_dwords == 0 {
            first + index
        } else {
            let at = index - 1;
            if at / 4 >= nsa_dwords {
                return None;
            }
            let word = instruction.words.get(2 + (at / 4) as usize)?;
            (word >> (8 * (at % 4))) & 0xff
        };
        addresses.push(register);
    }
    addresses
        .iter()
        .all(|&register| register < VECTOR_REGISTERS)
        .then_some(addresses)
}

/// Translates `image_bvh_intersect_ray` and `image_bvh64_intersect_ray`, per lane: the node's
/// sixteen words read through the guest-memory window, tested, and the four result words written.
/// In a module whose memory is exact, a node outside the window, or in another four-gigabyte span,
/// is recorded as an escape.
pub(super) fn intersect_ray<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let operands = checked_operands(instruction, name)?;
    let mut descriptor = [Id(0); 4];
    for (register, word) in (operands.descriptor..).zip(descriptor.iter_mut()) {
        *word = model.read_scalar(register);
    }
    let selectors = descriptor_selectors(model, descriptor);
    for lane in running_lanes(model) {
        let mut values = Vec::with_capacity(operands.addresses.len());
        for &register in &operands.addresses {
            let source = Operand::Vector(address_of(register));
            values.push(model.read_source(instruction, &source, lane)?);
        }
        let pointer_high = operands.wide_pointer.then(|| values[1]);
        let rest = &values[if operands.wide_pointer { 2 } else { 1 }..];
        let (low, high) = node_address_words(model, descriptor, (values[0], pointer_high));
        let kind = pointer_kind(model, values[0]);
        let high_word = pointer_high.unwrap_or_else(|| model.constant(0));
        // A device-addressed dispatch reads the node where it lies in guest memory (D964).
        if model.device_addressed() {
            let (node, placed) = device_node(model, (low, high), kind.box32, lane)?;
            let mut arithmetic = Emit {
                model: &mut *model,
                floats: Vec::new(),
            };
            let ray = ray_from(&mut arithmetic, rest, operands.a16);
            let valid = in_range(
                &mut arithmetic,
                descriptor,
                (values[0], high_word),
                kind.box32,
            );
            let result = intersect(&mut arithmetic, &node, &ray, (kind, selectors, valid));
            let missed = model.constant(NO_CHILD);
            for (register, value) in (operands.destination..).zip(result) {
                let value = model.select(placed, value, missed);
                model.write_vector_lane(register, lane, value);
            }
            continue;
        }
        if model.exact_memory() {
            let escaped = node_escapes(model, (low, high), kind.box32);
            model.note_escape(escaped, Some(lane));
        }
        let mut node = [low; NODE_WORDS];
        for (byte, word) in (0..).step_by(4).zip(node.iter_mut()) {
            let step = model.constant(byte);
            let address = model.add(low, step);
            *word = read_guarded(model, address);
        }
        let mut arithmetic = Emit {
            model: &mut *model,
            floats: Vec::new(),
        };
        let ray = ray_from(&mut arithmetic, rest, operands.a16);
        let valid = in_range(
            &mut arithmetic,
            descriptor,
            (values[0], high_word),
            kind.box32,
        );
        let result = intersect(&mut arithmetic, &node, &ray, (kind, selectors, valid));
        // A node the window does not hold reads as zeros, and a box of zeros can name child 0, so a
        // traversal loop in the guest's shader would revisit the root forever and the device
        // would be lost to its watchdog. An unreadable node answers no hit, which ends the loop.
        let placed = node_placed(model, (low, high), kind.box32);
        let missed = model.constant(NO_CHILD);
        for (register, value) in (operands.destination..).zip(result) {
            let value = model.select(placed, value, missed);
            model.write_vector_lane(register, lane, value);
        }
    }
    model.count();
    Ok(())
}

/// The instruction's four result words on the host, for the host evaluator's lane runs: the node
/// read through `read` at the guest address the descriptor and the lane's pointer name, and tested
/// in host arithmetic, the same test the translator emits. `vector` answers the lane's vector
/// registers by number. `None` where the instruction is not one the translator takes, or a
/// register, a descriptor word or a node word the test reads is unknown.
pub(crate) fn host_intersect_ray(
    instruction: &Instruction,
    name: &str,
    descriptor: [Option<u32>; 4],
    vector: &mut dyn FnMut(u32) -> Option<u32>,
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Option<[u32; 4]> {
    let operands = checked_operands(instruction, name).ok()?;
    let descriptor = [
        descriptor[0]?,
        descriptor[1]?,
        descriptor[2]?,
        descriptor[3]?,
    ];
    let values = operands
        .addresses
        .iter()
        .map(|&register| vector(register))
        .collect::<Option<Vec<u32>>>()?;
    let pointer = if operands.wide_pointer {
        u64::from(values[1]) << 32 | u64::from(values[0])
    } else {
        u64::from(values[0])
    };
    let rest = &values[if operands.wide_pointer { 2 } else { 1 }..];
    let (kind, options) = host_selectors(values[0], descriptor);
    let address = node_address(descriptor, pointer);
    // The words the test reads: a box of floats' 28, any other node's first 16.
    let last = if kind.box32 {
        LONG_LAST_BYTE
    } else {
        SHORT_LAST_BYTE
    };
    let mut node = [0; NODE_WORDS];
    for (byte, word) in (0..=last).step_by(4).zip(node.iter_mut()) {
        *word = read(address.wrapping_add(u64::from(byte)))?;
    }
    let ray = ray_from(&mut Host, rest, operands.a16);
    let high = if operands.wide_pointer { values[1] } else { 0 };
    let valid = in_range(&mut Host, descriptor, (values[0], high), kind.box32);
    Some(intersect(&mut Host, &node, &ray, (kind, options, valid)))
}

/// The ray from the address registers after the node pointer: extent and origin as floats, then
/// the directions as floats or, under `A16`, as halves two to a register in the order direction x,
/// y, z, inverse x, y, z (RDNA 2 instruction set, table 48).
fn ray_from<A: Arithmetic>(arithmetic: &mut A, registers: &[A::Word], a16: bool) -> Ray<A::Float> {
    let floats: Vec<A::Float> = registers
        .iter()
        .map(|&word| arithmetic.float_of(word))
        .collect();
    let directions: Vec<A::Float> = if a16 {
        registers[4..7]
            .iter()
            .flat_map(|&word| [(word, false), (word, true)])
            .map(|(word, high)| arithmetic.half_of(word, high))
            .collect()
    } else {
        floats[4..10].to_vec()
    };
    Ray {
        extent: floats[0],
        origin: [floats[1], floats[2], floats[3]],
        direction: [directions[0], directions[1], directions[2]],
        inverse: [directions[3], directions[4], directions[5]],
    }
}

/// The descriptor's box-sorting and return-mode bits as booleans.
fn descriptor_selectors<M: Model + ?Sized>(model: &mut M, descriptor: [Id; 4]) -> Options<Id, Id> {
    let sorting = model.constant(BOX_SORTING);
    let sorted = model.binary(op::BITWISE_AND, descriptor[1], sorting);
    let sorted = model.is_not_zero(sorted);
    let mode = model.constant(BARYCENTRIC_RETURN);
    let barycentrics = model.binary(op::BITWISE_AND, descriptor[3], mode);
    let barycentrics = model.is_not_zero(barycentrics);
    let shift = model.constant(GROW_SHIFT);
    let grow = model.binary(op::SHIFT_RIGHT_LOGICAL, descriptor[1], shift);
    let mask = model.constant(GROW_MASK);
    let grow = model.binary(op::BITWISE_AND, grow, mask);
    Options {
        sorted,
        barycentrics,
        grow,
    }
}

/// The node type the pointer's low bits name, as booleans.
fn pointer_kind<M: Model + ?Sized>(model: &mut M, pointer: Id) -> Kind<Id> {
    let mask = model.constant(NODE_TYPE_MASK);
    let kind = model.binary(op::BITWISE_AND, pointer, mask);
    let triangles = model.constant(TRIANGLE_TYPES);
    let triangle = model.compare(op::ULESS_THAN, kind, triangles);
    let box16 = model.constant(BOX16);
    let box16 = model.compare(op::IEQUAL, kind, box16);
    let box32 = model.constant(BOX32);
    let box32 = model.compare(op::IEQUAL, kind, box32);
    let which = [1, 2, 3].map(|index| {
        let index = model.constant(index);
        model.compare(op::IEQUAL, kind, index)
    });
    Kind {
        triangle,
        which,
        box16,
        box32,
    }
}

/// [`node_address`]'s low and high words: the base's `d0 << 8` and `d0 >> 24 | (d1 & 0xff) << 8`,
/// plus the pointer's `(low & !7) << 3` and `low >> 29 | high << 3`, with the carry between them.
fn node_address_words<M: Model + ?Sized>(
    model: &mut M,
    descriptor: [Id; 4],
    (pointer_low, pointer_high): (Id, Option<Id>),
) -> (Id, Id) {
    let (three, eight, twenty_four, twenty_nine) = (
        model.constant(3),
        model.constant(8),
        model.constant(24),
        model.constant(29),
    );
    let base_low = model.binary(op::SHIFT_LEFT_LOGICAL, descriptor[0], eight);
    let carried = model.binary(op::SHIFT_RIGHT_LOGICAL, descriptor[0], twenty_four);
    let high_mask = model.constant(BASE_HIGH_MASK);
    let top = model.binary(op::BITWISE_AND, descriptor[1], high_mask);
    let top = model.binary(op::SHIFT_LEFT_LOGICAL, top, eight);
    let base_high = model.binary(op::BITWISE_OR, carried, top);

    let type_bits = model.constant(!NODE_TYPE_MASK);
    let cleared = model.binary(op::BITWISE_AND, pointer_low, type_bits);
    let offset_low = model.binary(op::SHIFT_LEFT_LOGICAL, cleared, three);
    let mut offset_high = model.binary(op::SHIFT_RIGHT_LOGICAL, pointer_low, twenty_nine);
    if let Some(pointer_high) = pointer_high {
        let shifted = model.binary(op::SHIFT_LEFT_LOGICAL, pointer_high, three);
        offset_high = model.binary(op::BITWISE_OR, offset_high, shifted);
    }

    let low = model.add(base_low, offset_low);
    let wrapped = model.compare(op::ULESS_THAN, low, base_low);
    let (one, zero) = (model.constant(1), model.constant(0));
    let carry = model.select(wrapped, one, zero);
    let high = model.add(base_high, offset_high);
    let high = model.add(high, carry);
    (low, high)
}

/// Whether a node at `low`/`high` leaves the memory an exact module places: its first word, or the
/// last word its test reads (further for a box of floats, `long`), outside the window, or the node
/// in another four-gigabyte span than the window's.
fn node_escapes<M: Model + ?Sized>(model: &mut M, (low, high): (Id, Id), long: Id) -> Id {
    let placed = node_placed(model, (low, high), long);
    let bool_type = model.bool_type();
    let b = model.builder();
    let escaped = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, escaped.0, placed.0]);
    escaped
}

/// A device-addressed dispatch's node at `low`, `high` (D964): its words read by device address,
/// and whether a mapping holds it to its last word read - a box of floats' or a 64-byte node's, as
/// `long` says. A node no mapping holds is an escape, and answers no hit as an unplaced one does.
fn device_node<M: Model + ?Sized>(
    model: &mut M,
    (low, high): (Id, Id),
    long: Id,
    lane: u32,
) -> Result<([Id; NODE_WORDS], Id), TranslateError> {
    let mut node = [low; NODE_WORDS];
    let mut found = Vec::with_capacity(NODE_WORDS);
    for (byte, word) in (0..).step_by(4).zip(node.iter_mut()) {
        let (value, held) = super::device::read_word_found(model, (low, high), byte)?;
        *word = value;
        found.push(held);
    }
    let short_last = found[(SHORT_LAST_BYTE / 4) as usize];
    let long_last = found[(LONG_LAST_BYTE / 4) as usize];
    let last = super::select_bool(model, long, long_last, short_last);
    let placed = model.both(found[0], last);
    if model.exact_memory() {
        let bool_type = model.bool_type();
        let b = model.builder();
        let escaped = b.id();
        b.function(op::LOGICAL_NOT, &[bool_type.0, escaped.0, placed.0]);
        model.note_escape(escaped, Some(lane));
    }
    Ok((node, placed))
}

/// Whether a node at `low`/`high` lies wholly in the memory the window places: its first word and
/// the last word its test reads inside the window, in the window's four-gigabyte span.
fn node_placed<M: Model + ?Sized>(model: &mut M, (low, high): (Id, Id), long: Id) -> Id {
    let first = model.address_within_window(low);
    let (short_step, long_step) = (
        model.constant(SHORT_LAST_BYTE),
        model.constant(LONG_LAST_BYTE),
    );
    let last_step = model.select(long, long_step, short_step);
    let last = model.add(low, last_step);
    let last = model.address_within_window(last);
    let window_high = model.constant(model.memory_high());
    let same_span = model.compare(op::IEQUAL, high, window_high);
    let inside = model.both(first, last);
    model.both(inside, same_span)
}

#[cfg(test)]
mod tests {
    use super::{
        Host, NO_CHILD, NODE_WORDS, Ray, half_to_float, host_selectors, in_range, intersect,
        node_address,
    };

    /// Box sorting on, return mode 1 (barycentrics): the descriptor Mesa builds
    /// (`radv_nir_rt_common.c:582-585`).
    const DESCRIPTOR: [u32; 4] = [0, 1 << 31, u32::MAX, 0x8100_03ff];

    /// A ray from `origin` along `direction`, its inverse computed, out to `extent`.
    fn ray(origin: [f32; 3], direction: [f32; 3], extent: f32) -> Ray<f32> {
        Ray {
            extent,
            origin,
            direction,
            inverse: direction.map(|component| 1.0 / component),
        }
    }

    /// A box node of floats: four child pointers, each with its box's minimum and maximum.
    fn box32(children: [(u32, [f32; 3], [f32; 3]); 4]) -> [u32; NODE_WORDS] {
        let mut node = [0; NODE_WORDS];
        for (index, (pointer, low, high)) in children.into_iter().enumerate() {
            node[index] = pointer;
            for axis in 0..3 {
                node[4 + 6 * index + axis] = low[axis].to_bits();
                node[4 + 6 * index + 3 + axis] = high[axis].to_bits();
            }
        }
        node
    }

    /// The flags word of a node of one triangle: the second and third vertices' weights.
    const ONE_TRIANGLE: u32 = 9;

    /// A triangle node: its vertices, the first of the five a node holds, and its flags word.
    fn triangle(vertices: &[[f32; 3]], flags: u32) -> [u32; NODE_WORDS] {
        let mut node = [0; NODE_WORDS];
        for (vertex, coordinates) in vertices.iter().enumerate() {
            for (axis, value) in coordinates.iter().enumerate() {
                node[3 * vertex + axis] = value.to_bits();
            }
        }
        node[15] = flags;
        node
    }

    fn run(
        node: &[u32; NODE_WORDS],
        pointer: u32,
        ray: &Ray<f32>,
        descriptor: [u32; 4],
    ) -> [u32; 4] {
        let (kind, options) = host_selectors(pointer, descriptor);
        let valid = in_range(&mut Host, descriptor, (pointer, 0), kind.box32);
        intersect(&mut Host, node, ray, (kind, options, valid))
    }

    /// The node at 0x2000 with base 0x100 (256-byte units) and a pointer of offset 0x400 in
    /// 8-byte units: 0x10000 + 0x2000. The type bits are not part of the address, and the 64-bit
    /// form carries the pointer's high word into the address.
    #[test]
    fn a_node_address_is_the_base_plus_the_pointer_in_eight_byte_units() {
        let descriptor = [0x100, 0, 0, 0];
        assert_eq!(node_address(descriptor, 0x400 | 5), 0x1_2000);
        assert_eq!(node_address([0, 0x12, 0, 0], 0), 0x1200_0000_0000);
        assert_eq!(node_address([0, 0, 0, 0], 1 << 32), 1 << 35);
    }

    /// A ray down +z through two of four boxes answers those two, nearer first, then two misses.
    #[test]
    fn a_ray_hitting_two_boxes_answers_them_nearest_first() {
        let node = box32([
            (0x11, [5.0, 5.0, 0.0], [6.0, 6.0, 1.0]),
            (0x22, [-1.0, -1.0, 8.0], [1.0, 1.0, 9.0]),
            (0x33, [-1.0, -1.0, 2.0], [1.0, 1.0, 3.0]),
            (0x44, [-1.0, -1.0, -5.0], [1.0, 1.0, -4.0]),
        ]);
        let along_z = ray([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 100.0);
        assert_eq!(
            run(&node, 5, &along_z, DESCRIPTOR),
            [0x33, 0x22, NO_CHILD, NO_CHILD]
        );
        let unsorted = [DESCRIPTOR[0], 0, DESCRIPTOR[2], DESCRIPTOR[3]];
        assert_eq!(
            run(&node, 5, &along_z, unsorted),
            [NO_CHILD, 0x22, 0x33, NO_CHILD],
            "without box sorting the hits keep the node's order"
        );
        let short = ray([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 5.0);
        assert_eq!(
            run(&node, 5, &short, DESCRIPTOR),
            [0x33, NO_CHILD, NO_CHILD, NO_CHILD],
            "a box entered past the extent is a miss"
        );
    }

    /// A ray that misses every box, and one into an inactive (NaN) box, answer no child at all.
    #[test]
    fn a_miss_answers_no_children() {
        let mut node = box32([
            (0x11, [5.0, 5.0, 0.0], [6.0, 6.0, 1.0]),
            (0x22, [-1.0, -1.0, 8.0], [1.0, 1.0, 9.0]),
            (0x33, [2.0, 2.0, 2.0], [3.0, 3.0, 3.0]),
            (0x44, [-1.0, -1.0, -5.0], [1.0, 1.0, -4.0]),
        ]);
        let sideways = ray([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 100.0);
        assert_eq!(run(&node, 5, &sideways, DESCRIPTOR), [NO_CHILD; 4]);
        node[4 + 6] = f32::NAN.to_bits();
        let along_z = ray([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 100.0);
        assert_eq!(
            run(&node, 5, &along_z, DESCRIPTOR),
            [NO_CHILD; 4],
            "the one box on the ray is inactive"
        );
    }

    /// A box node of halves answers what the same boxes as floats do.
    #[test]
    fn a_box_of_halves_answers_as_the_box_of_floats() {
        // Halves: 0x3c00 1.0, 0xbc00 -1.0, 0x4000 2.0, 0x4200 3.0, 0x4800 8.0, 0x4880 9.0,
        // 0x4500 5.0, 0x4600 6.0, 0x0000 0.0, 0x7e00 NaN.
        let boxes: [[u16; 6]; 4] = [
            [0x4500, 0x4500, 0x0000, 0x4600, 0x4600, 0x3c00],
            [0xbc00, 0xbc00, 0x4800, 0x3c00, 0x3c00, 0x4880],
            [0xbc00, 0xbc00, 0x4000, 0x3c00, 0x3c00, 0x4200],
            [0x7e00; 6],
        ];
        let halves: Vec<u16> = boxes.iter().flatten().copied().collect();
        let mut node = [0; NODE_WORDS];
        node[..4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        for (index, pair) in halves.chunks(2).enumerate() {
            node[4 + index] = u32::from(pair[0]) | u32::from(pair[1]) << 16;
        }
        let along_z = ray([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 100.0);
        assert_eq!(
            run(&node, 4, &along_z, DESCRIPTOR),
            [0x33, 0x22, NO_CHILD, NO_CHILD]
        );
    }

    /// A ray down -z onto the triangle (0,0,-2), (4,0,-2), (0,4,-2) at (1,1): distance 2 and the
    /// second and third vertices' weights 1/4 each, as quotients of the answer's denominator. With z
    /// the major axis, x and y exchanged because the ray runs down it, the vertices less the origin
    /// are (-1,-1), (-1,3), (3,-1) at sheared depth 2, so the edge functions are 8, 4 and 4: `det`
    /// 16 and `t` 32.
    #[test]
    fn a_triangle_hit_answers_its_distance_and_barycentrics() {
        let flags = ONE_TRIANGLE | 0x5500_0000;
        let node = triangle(
            &[[0.0, 0.0, -2.0], [4.0, 0.0, -2.0], [0.0, 4.0, -2.0]],
            flags,
        );
        let down = ray([1.0, 1.0, 0.0], [0.0, 0.0, -1.0], 100.0);
        let answer = run(&node, 0, &down, DESCRIPTOR);
        assert_eq!(answer, [32.0f32, 16.0, 4.0, 4.0].map(f32::to_bits));
        let [t, det, i, j] = answer.map(f32::from_bits);
        assert_eq!(
            [t / det, i / det, j / det].map(f32::to_bits),
            [2.0f32, 0.25, 0.25].map(f32::to_bits)
        );

        let mode_zero = [DESCRIPTOR[0], DESCRIPTOR[1], DESCRIPTOR[2], 0x8000_03ff];
        let answer = run(&node, 0, &down, mode_zero);
        assert_eq!(
            answer[2..],
            [flags, 1],
            "return mode 0: the flags word whole and a hit"
        );
        let near = ray([1.0, 1.0, 0.0], [0.0, 0.0, -1.0], 1.5);
        assert_eq!(
            run(&node, 0, &near, mode_zero)[2..],
            [flags, 1],
            "the extent is the shader's to apply to a triangle"
        );
    }

    /// A ray beside the triangle, and one pointing away from it, miss: an infinite distance over
    /// one, and the weights the flags name all the same. Beside it at (3,3) down -z, the vertices
    /// less the origin, x and y exchanged, are (-3,-3), (-3,1), (1,-3): weights -8, 12 and 12, of
    /// mixed signs. Away from it up +z from (1,1), x and y kept, they are (-1,-1), (3,-1), (-1,3) at
    /// sheared depth -2: weights -8, -4 and -4, and `t` 32 against a `det` of -16, behind the origin.
    #[test]
    fn a_triangle_miss_answers_an_infinite_distance() {
        let node = triangle(
            &[[0.0, 0.0, -2.0], [4.0, 0.0, -2.0], [0.0, 4.0, -2.0]],
            ONE_TRIANGLE,
        );
        let miss = |i: f32, j: f32| [f32::INFINITY, 1.0, i, j].map(f32::to_bits);
        let beside = ray([3.0, 3.0, 0.0], [0.0, 0.0, -1.0], 100.0);
        assert_eq!(run(&node, 0, &beside, DESCRIPTOR), miss(12.0, 12.0));
        let away = ray([1.0, 1.0, 0.0], [0.0, 0.0, 1.0], 100.0);
        assert_eq!(run(&node, 0, &away, DESCRIPTOR), miss(-4.0, -4.0));
        let mode_zero = [DESCRIPTOR[0], DESCRIPTOR[1], DESCRIPTOR[2], 0x8000_03ff];
        assert_eq!(run(&node, 0, &away, mode_zero)[2..], [ONE_TRIANGLE, 0]);
    }

    /// Seen from behind - up +z from (1,1,-4), wound clockwise as the ray sees it - the triangle is
    /// hit with `t` and `det` both negative: the vertices less the origin, x and y kept, are
    /// (-1,-1), (3,-1), (-1,3) at sheared depth 2, weights -8, -4 and -4, `det` -16 and `t` -32.
    /// The quotients are the distance and barycentrics as from the front; the sign is the facing a
    /// shader culls by.
    #[test]
    fn a_triangle_seen_from_behind_answers_negative_distance_and_denominator() {
        let node = triangle(
            &[[0.0, 0.0, -2.0], [4.0, 0.0, -2.0], [0.0, 4.0, -2.0]],
            ONE_TRIANGLE,
        );
        let up = ray([1.0, 1.0, -4.0], [0.0, 0.0, 1.0], 100.0);
        let answer = run(&node, 0, &up, DESCRIPTOR);
        assert_eq!(answer, [-32.0f32, -16.0, -4.0, -4.0].map(f32::to_bits));
        let mode_zero = [DESCRIPTOR[0], DESCRIPTOR[1], DESCRIPTOR[2], 0x8000_03ff];
        assert_eq!(run(&node, 0, &up, mode_zero)[3], 1, "a hit either way");
    }

    /// A node's five vertices hold four triangles, the pointer's type bits naming which:
    /// (0,1,2), (1,3,2), (2,3,4) and (2,4,0). With vertices (0,0), (4,0), (0,4), (4,4) and (-4,8)
    /// at z = -2, a ray down -z at (3,3) lies in the second only. Its vertices less the origin, x and
    /// y exchanged, are (-3,1), (1,1), (1,-3): weights 4, 8 and 4, `det` 16 and `t` 32. Its flags
    /// byte, the word's second, `0x06` names the third vertex's weight first and the second's next.
    /// At (0.5,5) the ray lies in the third alone, at (-1,4) in the fourth alone, each at distance 2.
    #[test]
    fn a_pointer_s_type_names_one_of_four_triangles() {
        let vertices = [
            [0.0, 0.0, -2.0],
            [4.0, 0.0, -2.0],
            [0.0, 4.0, -2.0],
            [4.0, 4.0, -2.0],
            [-4.0, 8.0, -2.0],
        ];
        let node = triangle(&vertices, 0x0909_0609);
        let down = |x: f32, y: f32| ray([x, y, 0.0], [0.0, 0.0, -1.0], 100.0);
        let hits = |at: &Ray<f32>| {
            (0..4)
                .filter(|&kind| f32::from_bits(run(&node, kind, at, DESCRIPTOR)[0]).is_finite())
                .collect::<Vec<u32>>()
        };
        assert_eq!(hits(&down(3.0, 3.0)), [1]);
        assert_eq!(
            run(&node, 1, &down(3.0, 3.0), DESCRIPTOR),
            [32.0f32, 16.0, 4.0, 8.0].map(f32::to_bits),
            "the flags byte orders the weights"
        );
        assert_eq!(hits(&down(1.0, 1.0)), [0]);
        assert_eq!(hits(&down(0.5, 5.0)), [2]);
        assert_eq!(hits(&down(-1.0, 4.0)), [3]);
        for (kind, at) in [(2, down(0.5, 5.0)), (3, down(-1.0, 4.0))] {
            let [t, det, ..] = run(&node, kind, &at, DESCRIPTOR).map(f32::from_bits);
            assert_eq!((t / det).to_bits(), 2.0f32.to_bits(), "triangle {kind}");
        }
    }

    /// The descriptor's growth pushes each box's exit out by that many float steps. A ray along
    /// (1,1,0) from the origin enters the box x [2,3], y [0, 2 - 2^-23] at 2 (its x slab) and leaves
    /// one step before 2 (its y slab): a miss, and a hit once the exit is grown by a step.
    #[test]
    fn a_box_s_exit_grows_by_the_descriptor_s_steps() {
        let below_two = f32::from_bits(2.0f32.to_bits() - 1);
        let far = [5.0, 5.0, 5.0];
        let node = box32([
            (0x11, [2.0, 0.0, -1.0], [3.0, below_two, 1.0]),
            (0x22, far, [6.0, 6.0, 6.0]),
            (0x33, far, [6.0, 6.0, 6.0]),
            (0x44, far, [6.0, 6.0, 6.0]),
        ]);
        let diagonal = ray([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], 100.0);
        let unsorted = [DESCRIPTOR[0], 0, DESCRIPTOR[2], DESCRIPTOR[3]];
        assert_eq!(run(&node, 5, &diagonal, unsorted)[0], NO_CHILD);
        let grown = [DESCRIPTOR[0], 1 << 23, DESCRIPTOR[2], DESCRIPTOR[3]];
        assert_eq!(run(&node, 5, &diagonal, grown)[0], 0x11);
    }

    /// Sorted, two boxes the ray starts inside both enter at the origin, and keep the node's order;
    /// one the ray enters ahead of the origin comes after them.
    #[test]
    fn boxes_the_ray_starts_inside_sort_from_the_origin() {
        let node = box32([
            (0x11, [-1.0, -1.0, 4.0], [1.0, 1.0, 5.0]),
            (0x22, [-1.0, -1.0, -3.0], [1.0, 1.0, 3.0]),
            (0x33, [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]),
            (0x44, [5.0, 5.0, 5.0], [6.0, 6.0, 6.0]),
        ]);
        let along_z = ray([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 100.0);
        assert_eq!(
            run(&node, 5, &along_z, DESCRIPTOR),
            [0x22, 0x33, 0x11, NO_CHILD]
        );
    }

    /// A node past the descriptor's last node index, or a descriptor not a hierarchy's, answers no
    /// child in every word. The last index counts 64-byte slots: a box of floats at the last slot
    /// runs one past it.
    #[test]
    fn a_node_the_descriptor_does_not_hold_answers_no_children() {
        let far = [5.0, 5.0, 5.0];
        let node = box32([
            (0x11, [-1.0, -1.0, 2.0], [1.0, 1.0, 3.0]),
            (0x22, far, [6.0, 6.0, 6.0]),
            (0x33, far, [6.0, 6.0, 6.0]),
            (0x44, far, [6.0, 6.0, 6.0]),
        ]);
        let along_z = ray([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 100.0);
        // The last node index 3: a box of floats at slot 2 fits, at slot 3 runs past it.
        let three = [DESCRIPTOR[0], DESCRIPTOR[1], 3, 0x8100_0000];
        assert_eq!(run(&node, 2 << 3 | 5, &along_z, three)[0], 0x11);
        assert_eq!(run(&node, 3 << 3 | 5, &along_z, three), [NO_CHILD; 4]);
        let not_a_hierarchy = [DESCRIPTOR[0], DESCRIPTOR[1], DESCRIPTOR[2], 0x0100_03ff];
        assert_eq!(run(&node, 5, &along_z, not_a_hierarchy), [NO_CHILD; 4]);
        let ahead = triangle(
            &[[-1.0, -1.0, 2.0], [4.0, 0.0, 2.0], [0.0, 4.0, 2.0]],
            ONE_TRIANGLE,
        );
        assert_eq!(run(&ahead, 4 << 3, &along_z, three), [NO_CHILD; 4]);
        assert!(f32::from_bits(run(&ahead, 3 << 3, &along_z, three)[0]).is_finite());
    }

    /// An instance or procedural node answers no children: its result is not a test's.
    #[test]
    fn an_instance_node_answers_no_children() {
        let along_z = ray([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 100.0);
        for pointer in [6, 7] {
            assert_eq!(
                run(&[0; NODE_WORDS], pointer, &along_z, DESCRIPTOR),
                [NO_CHILD; 4]
            );
        }
    }

    /// Halves widen exactly: normal, subnormal, infinite and NaN.
    #[test]
    fn a_half_widens_to_its_value() {
        let widened = [0x3c00, 0xc200, 0x0001, 0x7c00].map(|half| half_to_float(half).to_bits());
        assert_eq!(
            widened,
            [1.0, -3.0, 2f32.powi(-24), f32::INFINITY].map(f32::to_bits)
        );
        assert!(half_to_float(0x7e00).is_nan());
    }
}
