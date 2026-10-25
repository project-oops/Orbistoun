//! A format load's conversion as its descriptor names it while the module runs (D967).
//!
//! A buffer descriptor the program computes - selected per lane from a table of records, say - is
//! not known when the module is translated, and neither is the format its fourth word names:
//! `FORMAT`, bits 18:12, and `DST_SEL_X..W`, bits 11:0 (`SQ_BUF_RSRC_WORD3`, `gfx10-rsrc.json`).
//! The module reads that word as it runs and decodes it. The format code picks a layout - each
//! component's width, and how its bits become a number - from the formats a format load the module
//! is translated for converts ([`super::descriptor_format`]), and each component is converted by
//! the same arithmetic ([`super::convert_field`]). A code those loads do not convert, a reserved
//! select, or a select of a component the format lacks cannot be answered: a lane reading one is
//! an escape, and the dispatch's result is refused (D950).

use std::collections::BTreeMap;

use orbistoun_shader::{BufferFormat, ComponentKind, FormatTable, Instruction};
use orbistoun_spirv::{Id, op};

use super::{
    BUFFER_FORMATS, DESCRIPTOR_FORMAT_SHIFT, FieldWidth, Model, convert_field, field_maximum,
    packed_format_admitted, pick, signed_kind,
};

/// The kinds a layout names by code, the code being the position here plus one; zero names a
/// format the module does not convert.
const KINDS: [ComponentKind; 7] = [
    ComponentKind::Unorm,
    ComponentKind::Snorm,
    ComponentKind::Uscaled,
    ComponentKind::Sscaled,
    ComponentKind::Uint,
    ComponentKind::Sint,
    ComponentKind::Float,
];

/// The bits of a layout's kind code, the lowest.
const KIND_BITS: u32 = 3;

/// The bits of each component's width above it, `x` first: wide enough for thirty-two.
const WIDTH_BITS: u32 = 6;

/// A format's layout as one word: its kind's code in the low [`KIND_BITS`], then each component's
/// width, `x` first, [`WIDTH_BITS`] each - zero for a component it lacks. Zero for a format a
/// format load the module is translated for does not convert either: one of components that are
/// not whole words, `packed_format_admitted`'s, or of more than four.
fn layout_key(instruction: &Instruction, format: &BufferFormat) -> u32 {
    let admitted = if format.is_plain_words() {
        format.components() <= 4
    } else {
        packed_format_admitted(instruction, format, true).is_ok()
    };
    let Some(kind) = KINDS.iter().position(|&kind| kind == format.kind) else {
        return 0;
    };
    if !admitted {
        return 0;
    }
    // `widths` lists components highest bits first, so `x` is the last.
    (0u32..)
        .zip(format.widths.iter().rev())
        .fold(kind as u32 + 1, |key, (component, &width)| {
            key | width << (KIND_BITS + WIDTH_BITS * component)
        })
}

/// A format the descriptor names, decoded as the module runs.
pub(super) struct Layout {
    /// The kind's code ([`KINDS`]); zero where the module does not convert the format.
    kind: Id,
    /// Each component's width in bits, `x` first; zero for one the format lacks.
    widths: [Id; 4],
    /// Each component's first bit in the element.
    bits: [Id; 4],
    /// The element's bytes: what a bounds check reads as its payload.
    pub(super) bytes: Id,
    /// Whether the format is an integer one, whose select of one writes `1` rather than `1.0`.
    integer: Id,
}

/// The layout of the format the fourth descriptor word `word3` names, chosen by its code among the
/// formats the module converts, each code compared once.
pub(super) fn layout<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    word3: Id,
) -> Layout {
    let formats = BUFFER_FORMATS.get_or_init(|| FormatTable::builtin().unwrap_or_default());
    let mut codes: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for format in formats.formats() {
        let key = layout_key(instruction, format);
        if key != 0 {
            codes.entry(key).or_default().push(format.code);
        }
    }
    let shift = model.constant(DESCRIPTOR_FORMAT_SHIFT);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word3, shift);
    let code_mask = model.constant(0x7F);
    let code = model.binary(op::BITWISE_AND, shifted, code_mask);
    let mut key = model.constant(0);
    for (layout, named) in codes {
        let mut is = None;
        for named in named {
            let named = model.constant(named);
            let same = model.compare(op::IEQUAL, code, named);
            is = Some(match is {
                Some(earlier) => model.either(earlier, same),
                None => same,
            });
        }
        if let Some(is) = is {
            let layout = model.constant(layout);
            key = model.select(is, layout, key);
        }
    }
    let field = |model: &mut M, shift: u32, mask: u32| {
        let shift = model.constant(shift);
        let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, key, shift);
        let mask = model.constant(mask);
        model.binary(op::BITWISE_AND, shifted, mask)
    };
    let kind = field(model, 0, (1 << KIND_BITS) - 1);
    let mut widths = [kind; 4];
    for (component, width) in (0u32..).zip(widths.iter_mut()) {
        *width = field(
            model,
            KIND_BITS + WIDTH_BITS * component,
            (1 << WIDTH_BITS) - 1,
        );
    }
    let mut bits = [model.constant(0); 4];
    for component in 1..4 {
        bits[component] = model.add(bits[component - 1], widths[component - 1]);
    }
    let total = model.add(bits[3], widths[3]);
    let seven = model.constant(7);
    let rounded = model.add(total, seven);
    let three = model.constant(3);
    let bytes = model.binary(op::SHIFT_RIGHT_LOGICAL, rounded, three);
    let mut integer = None;
    for named in [ComponentKind::Uint, ComponentKind::Sint] {
        let code = KINDS.iter().position(|&kind| kind == named).unwrap_or(0) as u32 + 1;
        let code = model.constant(code);
        let same = model.compare(op::IEQUAL, kind, code);
        integer = Some(match integer {
            Some(earlier) => model.either(earlier, same),
            None => same,
        });
    }
    Layout {
        kind,
        widths,
        bits,
        bytes,
        integer: integer.unwrap_or(kind),
    }
}

/// Each component of the element `words` hold - its first byte at bit 0 of the first - `x` first,
/// converted by `layout`'s kind, and zero where `outside` the buffer, as a format load the module
/// is translated for answers it ([`super::element_components`]).
pub(super) fn components<M: Model + ?Sized>(
    model: &mut M,
    layout: &Layout,
    words: &[Id; 4],
    outside: Id,
) -> [Id; 4] {
    let (zero, five, thirty_one, thirty_two) = (
        model.constant(0),
        model.constant(5),
        model.constant(31),
        model.constant(32),
    );
    let mut values = [zero; 4];
    for (component, value) in values.iter_mut().enumerate() {
        let (bit, width) = (layout.bits[component], layout.widths[component]);
        // No component spans a word (`packed_format_admitted`), so it lies in the word its first
        // bit does.
        let index = model.binary(op::SHIFT_RIGHT_LOGICAL, bit, five);
        let mut word = words[0];
        for (at, &other) in (1u32..).zip(&words[1..]) {
            let at = model.constant(at);
            let is = model.compare(op::IEQUAL, index, at);
            word = model.select(is, other, word);
        }
        let place = model.binary(op::BITWISE_AND, bit, thirty_one);
        let whole = model.compare(op::IEQUAL, width, thirty_two);
        // Zero- and sign-extended from the field, shifts kept under thirty-two: a whole word is
        // its field.
        let down = model.binary(op::SHIFT_RIGHT_LOGICAL, word, place);
        let mask = field_maximum(model, FieldWidth::Running(width), 0);
        let unsigned = model.binary(op::BITWISE_AND, down, mask);
        let unsigned = model.select(whole, word, unsigned);
        let above = model.add(place, width);
        let left = model.binary(op::ISUB, thirty_two, above);
        let left = model.binary(op::BITWISE_AND, left, thirty_one);
        let up = model.binary(op::SHIFT_LEFT_LOGICAL, word, left);
        let right = model.binary(op::ISUB, thirty_two, width);
        let right = model.binary(op::BITWISE_AND, right, thirty_one);
        let signed = model.binary(op::SHIFT_RIGHT_ARITHMETIC, up, right);
        let signed = model.select(whole, word, signed);
        let mut converted = unsigned;
        for (code, &kind) in (1u32..).zip(&KINDS) {
            let field = if signed_kind(kind) { signed } else { unsigned };
            let this = convert_field(model, field, FieldWidth::Running(width), kind);
            let code = model.constant(code);
            let is = model.compare(op::IEQUAL, layout.kind, code);
            converted = model.select(is, this, converted);
        }
        *value = pick(model, outside, zero, converted);
    }
    values
}

/// What channel `channel` of a format load writes - what its select in the fourth descriptor word
/// `word3` names among `components` and the constants - and whether that cannot be answered: a
/// reserved select, or one of a component the format lacks or the module does not convert.
/// `SQ_SEL_0` is zero and `SQ_SEL_1` one, `1` for an integer format and `1.0` for the others
/// (D738).
pub(super) fn channel<M: Model + ?Sized>(
    model: &mut M,
    layout: &Layout,
    word3: Id,
    channel: u32,
    components: &[Id; 4],
) -> (Id, Id) {
    let shift = model.constant(3 * channel);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word3, shift);
    let seven = model.constant(7);
    let select = model.binary(op::BITWISE_AND, shifted, seven);
    let (zero, one, float_one) = (
        model.constant(0),
        model.constant(1),
        model.constant(1.0f32.to_bits()),
    );
    let one_value = model.select(layout.integer, one, float_one);
    let is_one = model.compare(op::IEQUAL, select, one);
    let mut value = model.select(is_one, one_value, zero);
    let (two, three) = (model.constant(2), model.constant(3));
    let is_two = model.compare(op::IEQUAL, select, two);
    let is_three = model.compare(op::IEQUAL, select, three);
    let mut unanswered = model.either(is_two, is_three);
    for (named, (&component, &width)) in (4u32..).zip(components.iter().zip(&layout.widths)) {
        let named = model.constant(named);
        let is = model.compare(op::IEQUAL, select, named);
        let lacking = model.compare(op::IEQUAL, width, zero);
        let present = model.select(lacking, zero, component);
        value = model.select(is, present, value);
        let lacked = model.both(is, lacking);
        unanswered = model.either(unanswered, lacked);
    }
    (value, unanswered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instruction() -> Instruction {
        Instruction {
            offset: 0,
            length: 8,
            word: 0,
            second_word: None,
            words: Vec::new(),
            encoding: None,
            opcode: 0,
            operands: Vec::new(),
            operands_decoded: false,
        }
    }

    /// Every format a format load the module is translated for converts has a layout, and the rest
    /// none: the run-time decode answers exactly the formats the translated one does.
    #[test]
    fn a_layout_names_the_formats_a_translated_load_converts() {
        let formats = FormatTable::builtin().expect("formats");
        for format in formats.formats() {
            let translated = super::super::descriptor_format(
                &instruction(),
                format.code << DESCRIPTOR_FORMAT_SHIFT | 0xfac,
                1,
            )
            .is_ok();
            assert_eq!(
                layout_key(&instruction(), format) != 0,
                translated,
                "{}",
                format.name
            );
        }
    }

    /// A layout carries the kind and the widths `x` first: `10_11_11` has its eleven-bit `x` at
    /// bit 0, and `8_8_8_8_UNORM` four bytes.
    #[test]
    fn a_layout_lists_its_widths_from_x() {
        let formats = FormatTable::builtin().expect("formats");
        let named = |name: &str| {
            let format = formats
                .formats()
                .iter()
                .find(|format| format.name == name)
                .expect("a format");
            layout_key(&instruction(), format)
        };
        let float = 7;
        assert_eq!(
            named("BUF_FMT_10_11_11_FLOAT"),
            float | 11 << 3 | 11 << 9 | 10 << 15
        );
        assert_eq!(
            named("BUF_FMT_8_8_8_8_UNORM"),
            1 | 8 << 3 | 8 << 9 | 8 << 15 | 8 << 21
        );
        assert_eq!(named("BUF_FMT_32_FLOAT"), float | 32 << 3);
    }
}
