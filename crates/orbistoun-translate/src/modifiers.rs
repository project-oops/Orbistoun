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
//! `neg` and `abs` are applied - they are per-source, common, and cheap. The output
//! multiplier is **refused**, because implementing it wrongly is worse than not implementing
//! it and it has not appeared in a guest yet. An instruction carrying one is an error naming
//! it, never a silent drop.
//!
//! `clamp` is **read, and applied only where its meaning is known** (worklog 834): on a
//! long-form instruction whose result is a 32-bit float, in a stage whose `DX10_CLAMP` mode is
//! known, it clamps the result to `[0, 1]`. That is how a compiler folds GLSL's
//! `clamp(x, 0.0, 1.0)` into the instruction producing `x`, and Neverball's pixel shaders do. On
//! an integer result the same bit saturates instead, and on every other path it is still refused
//! by name.

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
    /// The output clamp. Only [`Modifiers::read_allowing_clamp`] reports it; [`Modifiers::read`]
    /// refuses it.
    pub clamp: bool,
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
        Self::read_with(instruction, has_scalar_destination, false)
    }

    /// Reads the modifiers as [`Modifiers::read`] does, but reports the clamp flag in
    /// [`Modifiers::clamp`] instead of refusing it - for the one caller that applies it or refuses
    /// it itself, knowing the instruction's result type and the stage's mode (worklog 834).
    ///
    /// # Errors
    ///
    /// A non-zero output multiplier.
    pub fn read_allowing_clamp(
        instruction: &Instruction,
        has_scalar_destination: bool,
    ) -> Result<Self, TranslateError> {
        Self::read_with(instruction, has_scalar_destination, true)
    }

    fn read_with(
        instruction: &Instruction,
        has_scalar_destination: bool,
        allow_clamp: bool,
    ) -> Result<Self, TranslateError> {
        // A short-form instruction has no second word and carries no modifiers. Absent
        // is the same as none here, which is the one place in this crate where that is
        // true - the flags genuinely do not exist in the short encoding.
        let Some(second) = instruction.second_word else {
            return Ok(Self::default());
        };

        let clamp = instruction.word & (1 << CLAMP_SHIFT) != 0;
        if clamp && !allow_clamp {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "this instruction clamps its result to [0, 1], which is not ",
                    "translated - ignoring it would compute a value outside the ",
                    "range the guest asked for and nothing downstream would notice"
                ),
            });
        }
        if (second >> OMOD_SHIFT) & OMOD_MASK != 0 {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "this instruction scales its result by an output multiplier, ",
                    "which is not translated - ignoring it would be wrong by a ",
                    "factor of two or four"
                ),
            });
        }

        let mut modifiers = Self {
            clamp,
            ..Self::default()
        };
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
