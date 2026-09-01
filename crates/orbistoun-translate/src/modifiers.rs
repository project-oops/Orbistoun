//! Source and output modifiers on the vector ALU's long-form encoding.
//!
//! # Why these get their own module
//!
//! They are not operands, so the operand solver never sees them, and they are not the
//! opcode, so the encoding table never sees them either. They sit in bits that both
//! layers correctly ignore - and every one of them changes the answer.
//!
//! `v_add_f32_e64 v0, v1, -v2` and `v_add_f32_e64 v0, v1, v2` differ by one bit and
//! compute different things. A translator that read the operands and stopped would emit
//! the second for both, and the shader would run, and every subtraction the compiler
//! expressed as an addition of a negated operand would come out with the wrong sign.
//! That is the single most likely way this crate could be quietly wrong at scale, which
//! is why the positions below were read off a reference assembler rather than
//! transcribed.
//!
//! # What is applied and what is refused
//!
//! `neg` and `abs` are applied - they are per-source, common, and cheap. `clamp` and the
//! output multiplier are **refused**, because implementing them wrongly is worse than
//! not implementing them and neither has appeared in a fixture yet. An instruction
//! carrying one is an error naming it, never a silent drop.

use orbistoun_shader::Instruction;

use crate::TranslateError;

/// Bit position of the first source's absolute-value flag, in the first word.
///
/// **Only in the sub-encoding that has one.** The other puts a scalar destination in
/// these same bits, and reading them there turns a carry-out register into a set of
/// modifiers - `vcc` is 106, whose low three bits are 010, so it presents as "the second
/// source is an absolute value". Which sub-encoding an opcode uses is a property of the
/// opcode and nothing in the instruction says it.
const ABS_SHIFT: u32 = 8;
/// Bit position of the clamp flag, in the first word.
const CLAMP_SHIFT: u32 = 15;
/// Bit position of the first source's negate flag, in the second word.
const NEG_SHIFT: u32 = 29;
/// Bit position of the output multiplier, in the second word.
const OMOD_SHIFT: u32 = 27;
/// Width of the output multiplier.
const OMOD_MASK: u32 = 0b11;

/// The modifiers on one long-form vector ALU instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Per-source negate, in operand order.
    pub negate: [bool; 3],
    /// Per-source absolute value, in operand order.
    pub absolute: [bool; 3],
}

impl Modifiers {
    /// Reads the modifiers of an instruction, refusing any this does not apply.
    ///
    /// # Errors
    ///
    /// The clamp flag or a non-zero output multiplier. Both change the result, and
    /// ignoring one produces a shader that computes something close to right - which is
    /// harder to find than one that refuses.
    ///
    /// `has_scalar_destination` says which sub-encoding this opcode uses. When it does,
    /// there are no absolute-value flags to read: those bits are the second destination.
    pub fn read(
        instruction: &Instruction,
        has_scalar_destination: bool,
    ) -> Result<Self, TranslateError> {
        // A short-form instruction has no second word and carries no modifiers. Absent
        // is the same as none here, which is the one place in this crate where that is
        // true - the flags genuinely do not exist in the short encoding.
        let Some(second) = instruction.second_word else {
            return Ok(Self::default());
        };

        if instruction.word & (1 << CLAMP_SHIFT) != 0 {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "this instruction clamps its result to [0, 1], which is not \
                         translated - ignoring it would compute a value outside the \
                         range the guest asked for and nothing downstream would notice",
            });
        }
        if (second >> OMOD_SHIFT) & OMOD_MASK != 0 {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "this instruction scales its result by an output multiplier, \
                         which is not translated - ignoring it would be wrong by a \
                         factor of two or four",
            });
        }

        let mut modifiers = Self::default();
        for source in 0..3 {
            let bit = u32::try_from(source).unwrap_or(0);
            // Negate is in the second word for both sub-encodings; absolute exists in
            // only one of them.
            modifiers.negate[source] = second & (1 << (NEG_SHIFT + bit)) != 0;
            modifiers.absolute[source] =
                !has_scalar_destination && instruction.word & (1 << (ABS_SHIFT + bit)) != 0;
        }
        Ok(modifiers)
    }

    /// Whether any modifier applies to a source.
    pub const fn touches(&self, source: usize) -> bool {
        source < 3 && (self.negate[source] || self.absolute[source])
    }
}
