//! The SDWA form of any one- or two-source vector instruction (D979): the instruction itself, over
//! the parts of its sources its modifier word selects, its result placed into the part of its
//! destination the word names.
//!
//! The word's fields are each source's select (a byte, a word or the whole dword), zero- or
//! sign-extended; each source's negate and absolute value, on a float or bit-pattern source; the
//! result's clamp and output multiplier, on a float result; and where the result lands, with what
//! fills the destination's other bits (AMD's published RDNA 2 instruction set, SDWA; LLVM's
//! `AMDGPUModifierSyntax` for each field's meaning). None of them depends on what the instruction
//! computes, so they are applied around it once, here, rather than restated in each instruction's
//! translation: the selected and modified sources are staged where the plain short form reads
//! them, the plain form runs, and its result is modified and placed.
//!
//! The first source is staged in the register past every register an instruction names
//! ([`DPP_STAGING_REGISTER`]), and a second in the destination itself, whose old bits are read
//! first: no source the instruction reads is its destination unless it accumulates into it, and
//! an accumulating form has no SDWA encoding.

use orbistoun_shader::{Instruction, Operand};
use orbistoun_spirv::{Id, op};

use super::{
    DPP_STAGING_REGISTER, Model, SDWA_DWORD, SDWA_MARKER, SdwaFields, clamp_unit, output_scale,
    running_lanes, sdwa_operand, sdwa_place, sdwa_select,
};
use crate::TranslateError;

/// What an operand of an instruction holds, as far as its modifiers are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A signed or unsigned integer (`i32`, `u16`, `u24`, a conversion's `ubyte0`): a select may
    /// sign-extend it, and a negate or an absolute value is not defined on it.
    Integer {
        /// Its width in bits.
        bits: u32,
    },
    /// A bit pattern (`b32`, `b16`): carried as it is, so a negate or an absolute value is an
    /// operation on its top bit.
    Bits {
        /// Its width in bits.
        bits: u32,
    },
    /// An IEEE float (`f32`, `f16`).
    Float {
        /// Its width in bits.
        bits: u32,
    },
}

impl Kind {
    /// The kind a name's type token gives: `None` for a token that is no type.
    fn of(token: &str) -> Option<Self> {
        let (class, width) = token.split_at_checked(1)?;
        let bits = width.parse::<u32>().ok()?;
        match class {
            "f" => Some(Self::Float { bits }),
            "b" => Some(Self::Bits { bits }),
            "i" | "u" => Some(Self::Integer { bits }),
            _ => None,
        }
    }

    /// Its width in bits.
    const fn bits(self) -> u32 {
        match self {
            Self::Integer { bits } | Self::Bits { bits } | Self::Float { bits } => bits,
        }
    }

    /// Whether a negate or an absolute value is defined on it: a float or a bit pattern, whose top
    /// bit they clear and flip.
    pub(crate) const fn signed_by_bit(self) -> bool {
        !matches!(self, Self::Integer { .. })
    }
}

/// The kinds of an instruction's result and of its two sources, read off its name: a conversion
/// `v_cvt_<to>_<from>` names its result first and its source last, and any other instruction
/// names one type for all of them (`v_mul_u32_u24` its result and then its sources'). A token that
/// names no type - `ubyte0`, `i4` - is an integer source. `v_ldexp_f16`'s second source is an
/// integer exponent, and a packing conversion's result is two halves: a bit pattern.
///
/// `None` for an instruction with no typed operand, or a sixty-four-bit one, which has no SDWA or
/// DPP form (LLVM's `VOPProfile`, `HasExtSDWA` and `HasExtDPP` are false for 64-bit operands).
pub(crate) fn kinds(name: &str) -> Option<(Kind, [Kind; 2])> {
    let base = name.strip_suffix("_e32").unwrap_or(name);
    let tokens: Vec<&str> = base.split('_').skip(1).collect();
    let typed: Vec<Kind> = tokens.iter().filter_map(|token| Kind::of(token)).collect();
    let result = *typed.first()?;
    if typed.iter().any(|kind| kind.bits() > 32) {
        return None;
    }
    let last = tokens.last().copied().unwrap_or_default();
    let source = Kind::of(last).unwrap_or(Kind::Integer { bits: 32 });
    let result = if tokens.contains(&"pk") || tokens.contains(&"pkrtz") {
        Kind::Bits { bits: 32 }
    } else {
        result
    };
    let second = if base.starts_with("v_ldexp_") {
        Kind::Integer { bits: 32 }
    } else {
        source
    };
    Some((result, [source, second]))
}

/// Why a vector instruction of `family` named `name` has no SDWA form this translates, or `None`
/// where it has one: a one- or two-source instruction with a typed operand of at most 32 bits that
/// neither accumulates into its destination nor takes a literal, writes a scalar, or indexes or
/// swaps registers (LLVM's `VOP1Instructions.td` and `VOP2Instructions.td` give each of these no
/// SDWA form on this generation; `llvm-mc` refuses `v_fmac_f32_sdwa`). A compare (VOPC) takes the
/// compare path ([`super::sdwa_compare`]).
pub(crate) fn refusal(family: &str, name: &str) -> Option<&'static str> {
    const NO_FORM: &[&str] = &[
        "mac",
        "fmac",
        "dot2c",
        "mamk",
        "maak",
        "madmk",
        "madak",
        "movrel",
        "swap",
        "readfirstlane",
    ];
    if family != "VOP1" && family != "VOP2" {
        return Some("an SDWA form outside the one- and two-source vector families");
    }
    if NO_FORM.iter().any(|part| name.contains(part)) {
        return Some(concat!(
            "an SDWA form of an instruction that accumulates into its destination, takes a ",
            "literal, writes a scalar or moves registers by index, which has none"
        ));
    }
    if kinds(name).is_none() {
        return Some("an SDWA form of an instruction with no typed operand of at most 32 bits");
    }
    None
}

/// The place in `instruction`'s operands of the first source its modifier word replaces: the
/// operand the marker `marker` decodes to, which the operand table leaves unmapped.
pub(crate) fn marked_source(instruction: &Instruction, marker: u32) -> Option<usize> {
    instruction.operands.iter().position(
        |operand| matches!(operand, Operand::Unrecognised(code) if u32::from(*code) == marker),
    )
}

/// `value` under a source's negate and absolute value, `flags` bit 0 and bit 1, as operations on
/// the top bit of a `kind` operand: cleared for the absolute value, then flipped for the negate.
pub(crate) fn sign_flags<M: Model + ?Sized>(
    model: &mut M,
    value: Id,
    flags: u32,
    kind: Kind,
) -> Id {
    let top = 1_u32 << (kind.bits().clamp(1, 32) - 1);
    let mut value = value;
    if flags & 0b10 != 0 {
        let magnitude = model.constant(!top);
        value = model.binary(op::BITWISE_AND, value, magnitude);
    }
    if flags & 0b01 != 0 {
        let sign = model.constant(top);
        value = model.binary(op::BITWISE_XOR, value, sign);
    }
    value
}

/// The output multiplier (`omod`, 1 two, 2 four, 3 a half) and then the clamp to `[0, 1]` on a
/// float result of `kind`, a NaN as `nan_to_zero` says: a half widened, scaled and clamped in
/// single precision, and narrowed again. Scaling by a power of two and clamping to two representable
/// bounds commute with the rounding, so the result is the one the hardware rounds once.
fn output_modifiers<M: Model + ?Sized>(
    model: &mut M,
    result: Id,
    kind: Kind,
    (scale, clamp): (Option<u32>, Option<bool>),
) -> Id {
    if scale.is_none() && clamp.is_none() {
        return result;
    }
    let half = kind.bits() == 16;
    let mut value = if half {
        let low = model.constant(0xFFFF);
        let field = model.binary(op::BITWISE_AND, result, low);
        model.half_to_float_bits(field)
    } else {
        result
    };
    if let Some(bits) = scale {
        let factor = model.constant(bits);
        value = model.f32_binary(op::FMUL, value, factor);
    }
    if let Some(nan_to_zero) = clamp {
        value = clamp_unit(model, value, nan_to_zero);
    }
    if half {
        model.float_bits_to_half(value)
    } else {
        value
    }
}

/// `instruction`, a one- or two-source vector instruction `name` whose SDWA modifier word is
/// `word`, translated over the parts of its sources the word selects into the part of its
/// destination the word names (D979).
///
/// Each source is read whole - a vector register, or for the first a scalar register, an inline
/// constant or a named register where bit 23 says, and for the second a scalar register where bit
/// 31 says - then its select (18:16, 26:24) picks a byte or a word of it, sign-extended where 19 or
/// 27 says, and its negate (20, 28) and absolute value (21, 29) flip and clear its top bit. The
/// result is scaled (15:14) and clamped (13) where it is a float, and placed into the destination
/// select (10:8): the part from the result's low bits, the rest zero, the part's sign above it with
/// zero below, or the destination's old bits (12:11). A sixteen-bit result fills a whole-dword
/// destination zero-extended.
///
/// # Errors
///
/// Refused where [`refusal`] refuses the instruction, a source's operand is not one an SDWA field
/// can name, an integer source is negated, an integer or bit-pattern result is clamped or scaled,
/// a clamp has no `DX10_CLAMP` mode to follow, or the instruction itself is.
pub(crate) fn sdwa_form<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (family, name): (&str, &str),
    word: u32,
) -> Result<(), TranslateError> {
    let Admitted {
        kinds: (result_kind, source_kinds),
        destination,
        first,
        second,
        source_flags,
        output: (scale, clamp),
    } = admitted(model, instruction, (family, name), word)?;
    let fields = SdwaFields::read(word);
    let sources = [
        Some(sdwa_operand(instruction, fields.src0, fields.src0_scalar)?),
        match second {
            Some((_, register)) => Some(sdwa_operand(
                instruction,
                u32::from(register),
                fields.src1_scalar,
            )?),
            None => None,
        },
    ];
    let staged_first = DPP_STAGING_REGISTER;
    let narrow_result = result_kind.bits() == 16;
    // Whether the result needs anything after the plain form: a part of the destination, a
    // sixteen-bit result's zero high half, or an output modifier.
    let placed =
        fields.dst_sel != SDWA_DWORD || narrow_result || clamp.is_some() || scale.is_some();

    // Each running lane's sources, selected and modified, read before any is written; and its
    // destination as it was, where the result is placed into part of it.
    let mut kept = Vec::new();
    for lane in running_lanes(model) {
        let old = if placed {
            Some(model.read_source(instruction, &Operand::Vector(destination), lane)?)
        } else {
            None
        };
        let mut values = [None, None];
        for (index, source) in sources.iter().enumerate() {
            let Some(source) = source else { continue };
            let value = model.read_source(instruction, source, lane)?;
            let value = sdwa_select(model, instruction, value, fields.select[index])?;
            values[index] = Some(sign_flags(
                model,
                value,
                source_flags[index],
                source_kinds[index],
            ));
        }
        if let Some(value) = values[0] {
            model.write_vector_lane(staged_first, lane, value);
        }
        if let Some(value) = values[1] {
            model.write_vector_lane(u32::from(destination), lane, value);
        }
        kept.push((lane, old));
    }

    // The plain short form over the staged sources. Its first-source field names an ordinary
    // register, so the form is not seen again; the operand list is what translation reads.
    let mut plain = instruction.clone();
    plain.operands[first] = Operand::Vector(staged_first as u16);
    if let Some((at, _)) = second {
        plain.operands[at] = Operand::Vector(destination);
    }
    plain.word = (plain.word & !0x1ff) | 0x100;
    plain.second_word = None;
    super::instruction(model, &plain)?;

    if placed {
        for (lane, old) in kept {
            let Some(old) = old else { continue };
            let result = model.read_source(&plain, &Operand::Vector(destination), lane)?;
            let result = if narrow_result {
                let low = model.constant(0xFFFF);
                model.binary(op::BITWISE_AND, result, low)
            } else {
                result
            };
            let result = output_modifiers(model, result, result_kind, (scale, clamp));
            let value = sdwa_place(model, instruction, result, old, fields)?;
            model.write_vector_lane(u32::from(destination), lane, value);
        }
    }
    Ok(())
}

/// What [`admitted`] finds of an SDWA form [`sdwa_form`] translates.
struct Admitted {
    /// The result's kind and the sources'.
    kinds: (Kind, [Kind; 2]),
    /// The destination register.
    destination: u16,
    /// The place of the first source in the operand list, and the second's with its register.
    first: usize,
    second: Option<(usize, u16)>,
    /// Each source's negate (bit 0) and absolute value (bit 1).
    source_flags: [u32; 2],
    /// The output multiplier's factor and the clamp's `DX10_CLAMP` mode, where each applies.
    output: (Option<u32>, Option<bool>),
}

/// Whether an SDWA form of `name` with `word` is one [`sdwa_form`] translates, and what it reads
/// and writes where it is.
fn admitted<M: Model + ?Sized>(
    model: &M,
    instruction: &Instruction,
    (family, name): (&str, &str),
    word: u32,
) -> Result<Admitted, TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    if let Some(detail) = refusal(family, name) {
        return Err(refuse(detail));
    }
    let Some((result_kind, source_kinds)) = kinds(name) else {
        return Err(refuse(
            "an SDWA form of an instruction with no typed operand",
        ));
    };
    let (Some(Operand::Vector(destination)), Some(first)) = (
        instruction.operands.first(),
        marked_source(instruction, SDWA_MARKER),
    ) else {
        return Err(refuse(
            "an SDWA form whose destination is not a vector register, or whose first source is not its word's",
        ));
    };
    let destination = *destination;
    // A two-source instruction's second source is its register field, the operand after the first.
    let second = match (family, instruction.operands.get(first + 1)) {
        ("VOP2", Some(Operand::Vector(register))) => Some((first + 1, *register)),
        ("VOP2", _) => {
            return Err(refuse(
                "an SDWA form whose second source is not its register field",
            ));
        }
        _ => None,
    };
    let source_flags = [word >> 20 & 0b11, word >> 28 & 0b11];
    let read = if second.is_some() { 2 } else { 1 };
    if (0..read).any(|index| source_flags[index] != 0 && !source_kinds[index].signed_by_bit()) {
        return Err(refuse(
            "an SDWA form negates or takes the absolute value of an integer source",
        ));
    }
    let (clamp, omod) = (word >> 13 & 1 != 0, word >> 14 & 0b11);
    if (clamp || omod != 0) && !matches!(result_kind, Kind::Float { .. }) {
        return Err(refuse(
            "an SDWA form clamps or scales an integer or bit-pattern result, which is not translated",
        ));
    }
    let clamp = if clamp {
        Some(model.dx10_clamp().ok_or_else(|| {
            refuse(concat!(
                "an SDWA form clamps its result to [0, 1], and what that does to a NaN is the ",
                "stage's DX10_CLAMP mode, which this translation was not given"
            ))
        })?)
    } else {
        None
    };
    Ok(Admitted {
        kinds: (result_kind, source_kinds),
        destination,
        first,
        second,
        source_flags,
        output: (output_scale(omod), clamp),
    })
}

#[cfg(test)]
mod tests {
    use super::{Kind, kinds, refusal};

    /// The kinds read off a name: a conversion's result and source, one type for every operand of
    /// an arithmetic instruction, and the exceptions.
    #[test]
    fn an_instructions_kinds_are_read_off_its_name() {
        let float = |bits| Kind::Float { bits };
        let integer = |bits| Kind::Integer { bits };
        let bits = |bits| Kind::Bits { bits };
        assert_eq!(
            kinds("v_min_u32_e32"),
            Some((integer(32), [integer(32); 2]))
        );
        assert_eq!(
            kinds("v_mul_u32_u24_e32"),
            Some((integer(32), [integer(24); 2]))
        );
        assert_eq!(kinds("v_add_f16_e32"), Some((float(16), [float(16); 2])));
        assert_eq!(
            kinds("v_cvt_f32_ubyte0_e32"),
            Some((float(32), [integer(32); 2]))
        );
        assert_eq!(
            kinds("v_cvt_f16_f32_e32"),
            Some((float(16), [float(32); 2]))
        );
        assert_eq!(
            kinds("v_cvt_i32_f32_e32"),
            Some((integer(32), [float(32); 2]))
        );
        assert_eq!(kinds("v_mov_b32_e32"), Some((bits(32), [bits(32); 2])));
        assert_eq!(
            kinds("v_ldexp_f16_e32"),
            Some((float(16), [float(16), integer(32)]))
        );
        assert_eq!(
            kinds("v_cvt_pkrtz_f16_f32_e32"),
            Some((bits(32), [float(32); 2]))
        );
        assert_eq!(
            kinds("v_cvt_f64_f32_e32"),
            None,
            "a 64-bit operand has no SDWA form"
        );
        assert_eq!(kinds("v_nop"), None);
    }

    /// The instructions with no SDWA form are refused by what they are, not listed one by one.
    #[test]
    fn an_instruction_with_no_sdwa_form_is_refused() {
        assert_eq!(refusal("VOP2", "v_min_u32_e32"), None);
        assert_eq!(refusal("VOP1", "v_cvt_f32_ubyte0_e32"), None);
        assert!(refusal("VOP2", "v_fmac_f32_e32").is_some());
        assert!(refusal("VOP2", "v_fmamk_f32").is_some());
        assert!(refusal("VOP1", "v_readfirstlane_b32").is_some());
        assert!(refusal("VOP1", "v_movrels_b32_e32").is_some());
        assert!(refusal("VOPC", "v_cmp_lt_f32_e32").is_some());
    }
}
