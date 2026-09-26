//! What an instruction operates on.
//!
//! A source operand field is not a register index: by value it selects a scalar, vector
//! or special register, a small inline integer, an inline float, or a trailing literal.
//! The scheme is uniform across encodings and lives once in `data/operands.toml`. Scalar
//! destinations use it too (scalar registers stop at 101 and higher codes are special
//! registers, so `vcc` is code 106); only vector destinations are a plain index. A wrong
//! boundary yields a plausible register where a constant belongs, so every operand is
//! checked against a reference disassembler in `tests/differential.rs`.

use serde::Deserialize;

use crate::ShaderError;

/// Where an operand's bits sit, and how to read them.
#[derive(Debug, Clone, Deserialize)]
pub struct OperandSlot {
    /// Name from the specification, where one is known.
    /// Empty for a field the solver recovered from observation rather than transcribed
    /// from the specification.
    #[serde(default)]
    pub name: String,
    /// Which dword of the instruction holds the field.
    ///
    /// Zero for the first. Sixty-four-bit encodings keep most operands in the second word.
    #[serde(default)]
    pub word: u32,
    /// Bit position of the field's low bit.
    pub shift: u32,
    /// Width of the field in bits.
    pub width: u32,
    /// How the field's value should be interpreted.
    pub kind: SlotKind,
    /// Multiplier applied to the raw value before it is interpreted.
    ///
    /// Some fields store a register group index rather than the register: a base naming
    /// an aligned pair stores half the register number.
    #[serde(default)]
    pub scale: Option<u32>,
    /// The operand's text, for a slot of kind [`SlotKind::Implicit`].
    ///
    /// Ignored for every other kind, where the value comes from the instruction.
    #[serde(default)]
    pub implicit: Option<String>,
}

/// How an operand field's raw value is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    /// The unified source-operand space in `data/operands.toml`.
    Source,
    /// A plain vector register index.
    ///
    /// Vector destinations are a direct index (`v0` is zero), unlike scalar fields.
    Vgpr,
    /// A literal value held in the instruction: a memory offset, a branch target.
    ///
    /// Not a register, so it does not go through the operand numbering.
    Immediate,
    /// An operand the encoding does not carry at all.
    ///
    /// The 32-bit comparison forms write the condition mask only, so `vcc` is printed but
    /// occupies no bits. The solver emits this only when no field explains the operand and
    /// it is textually identical in every sample, and the assembler refuses any other value
    /// in that position.
    Implicit,
}

impl OperandSlot {
    /// Extracts this field's value, applying any scale.
    pub const fn extract(&self, word: u32) -> u32 {
        let mask = if self.width >= 32 {
            u32::MAX
        } else {
            (1u32 << self.width) - 1
        };
        let raw = (word >> self.shift) & mask;
        match self.scale {
            Some(factor) => raw.wrapping_mul(factor),
            None => raw,
        }
    }
}

/// One decoded operand.
///
/// Inline floats are carried as names rather than `f32`: `f32` has no total equality,
/// and a translator needs the exact bit pattern rather than a parsed value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    /// Scalar register.
    Scalar(u16),
    /// Vector register.
    Vector(u16),
    /// A special register or an inline float, by its documented name.
    Named(String),
    /// An inline integer constant.
    Integer(i64),
    /// A 32-bit literal that followed the instruction, already substituted.
    Literal(u32),
    /// A value held directly in the instruction rather than naming a register.
    Immediate(i64),
    /// A code no range in the table covers.
    ///
    /// An unmapped code is a gap in the table, reported rather than read as a register.
    Unrecognised(u16),
}

impl core::fmt::Display for Operand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Scalar(n) => write!(f, "s{n}"),
            Self::Vector(n) => write!(f, "v{n}"),
            Self::Named(name) => write!(f, "{name}"),
            Self::Integer(v) => write!(f, "{v}"),
            Self::Immediate(v) => write!(f, "{v:#x}"),
            Self::Literal(v) => write!(f, "{v:#x}"),
            Self::Unrecognised(code) => write!(f, "<unmapped:{code}>"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RangeKind {
    Sgpr,
    Vgpr,
    Named,
    Integer,
    NegativeInteger,
    Literal,
}

#[derive(Debug, Clone, Deserialize)]
struct CodeRange {
    first: u32,
    last: u32,
    kind: RangeKind,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    origin: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct TableFile {
    #[serde(default)]
    operand_code: Vec<CodeRange>,
}

/// The unified source-operand space.
#[derive(Debug, Clone, Default)]
pub struct OperandTable {
    ranges: Vec<CodeRange>,
}

impl OperandTable {
    /// Parses a table from TOML.
    ///
    /// Refuses overlapping ranges, which would make the file's order decide which range
    /// claims a code.
    pub fn load(toml_text: &str) -> Result<Self, ShaderError> {
        let file: TableFile =
            toml::from_str(toml_text).map_err(|e| ShaderError::Table(e.to_string()))?;
        if file.operand_code.is_empty() {
            return Err(ShaderError::Table(
                "the operand table declares no ranges".into(),
            ));
        }

        let mut ranges = file.operand_code;
        for range in &ranges {
            if range.first > range.last {
                return Err(ShaderError::Table(format!(
                    "operand range {}..{} is inverted",
                    range.first, range.last
                )));
            }
            if range.kind == RangeKind::Named && range.name.is_none() {
                return Err(ShaderError::Table(format!(
                    "operand range {}..{} is named but has no name",
                    range.first, range.last
                )));
            }
        }
        ranges.sort_by_key(|r| r.first);
        for pair in ranges.windows(2) {
            if pair[0].last >= pair[1].first {
                return Err(ShaderError::Table(format!(
                    "operand ranges {}..{} and {}..{} overlap",
                    pair[0].first, pair[0].last, pair[1].first, pair[1].last
                )));
            }
        }
        Ok(Self { ranges })
    }

    /// The built-in table.
    pub fn builtin() -> Result<Self, ShaderError> {
        Self::load(include_str!("../data/operands.toml"))
    }

    /// Whether a code selects a trailing literal.
    ///
    /// Asked before decoding, because the literal's value is not in the instruction.
    pub fn selects_literal(&self, code: u32) -> bool {
        self.range_for(code)
            .is_some_and(|r| r.kind == RangeKind::Literal)
    }

    /// Interprets a source-operand code.
    ///
    /// `literal` supplies the trailing dword when the code calls for one. `None` where a
    /// literal is expected yields `Unrecognised`, since it means the caller and decoder
    /// disagree about the instruction's length.
    pub fn classify(&self, code: u32, literal: Option<u32>) -> Operand {
        let Some(range) = self.range_for(code) else {
            return Operand::Unrecognised(u16::try_from(code).unwrap_or(u16::MAX));
        };
        let origin = range.origin.unwrap_or(range.first);
        match range.kind {
            RangeKind::Sgpr => Operand::Scalar(u16::try_from(code - origin).unwrap_or(u16::MAX)),
            RangeKind::Vgpr => Operand::Vector(u16::try_from(code - origin).unwrap_or(u16::MAX)),
            RangeKind::Named => range
                .name
                .clone()
                .map_or(Operand::Unrecognised(u16::MAX), Operand::Named),
            RangeKind::Integer => Operand::Integer(i64::from(code) - i64::from(origin)),
            RangeKind::NegativeInteger => Operand::Integer(-(i64::from(code) - i64::from(origin))),
            RangeKind::Literal => literal.map_or(
                Operand::Unrecognised(u16::try_from(code).unwrap_or(u16::MAX)),
                Operand::Literal,
            ),
        }
    }

    fn range_for(&self, code: u32) -> Option<&CodeRange> {
        self.ranges
            .iter()
            .find(|r| code >= r.first && code <= r.last)
    }
}

#[cfg(test)]
mod tests {
    use super::{Operand, OperandTable};

    fn table() -> OperandTable {
        OperandTable::builtin().expect("builtin operand table")
    }

    /// Codes below the special registers are scalar registers.
    #[test]
    fn low_codes_are_scalar_registers() {
        assert_eq!(table().classify(0, None), Operand::Scalar(0));
        assert_eq!(table().classify(37, None), Operand::Scalar(37));
    }

    /// A vector register index is the code minus 256.
    #[test]
    fn high_codes_are_vector_registers_counted_from_their_origin() {
        assert_eq!(table().classify(256, None), Operand::Vector(0));
        assert_eq!(table().classify(260, None), Operand::Vector(4));
    }

    /// Code 128 is the integer zero, not scalar register 128.
    #[test]
    fn the_inline_zero_is_a_constant_not_a_register() {
        assert_eq!(table().classify(128, None), Operand::Integer(0));
        assert_eq!(table().classify(129, None), Operand::Integer(1));
        assert_eq!(table().classify(192, None), Operand::Integer(64));
    }

    /// Negative inline constants count down from -1.
    #[test]
    fn negative_inline_constants_count_downwards() {
        assert_eq!(table().classify(193, None), Operand::Integer(-1));
        assert_eq!(table().classify(208, None), Operand::Integer(-16));
    }

    /// The condition and execution masks decode to their names.
    #[test]
    fn special_registers_keep_their_names() {
        assert_eq!(
            table().classify(106, None),
            Operand::Named("vcc_lo".to_owned())
        );
        assert_eq!(
            table().classify(126, None),
            Operand::Named("exec_lo".to_owned())
        );
    }

    /// Inline floats decode to their names.
    #[test]
    fn inline_floats_are_named_rather_than_parsed() {
        assert_eq!(
            table().classify(242, None),
            Operand::Named("1.0".to_owned())
        );
        assert_eq!(
            table().classify(243, None),
            Operand::Named("-1.0".to_owned())
        );
    }

    /// A literal code takes the dword that followed the instruction.
    #[test]
    fn a_literal_code_takes_the_value_that_followed_the_instruction() {
        let table = table();
        assert!(table.selects_literal(255));
        assert_eq!(
            table.classify(255, Some(0x1234_5678)),
            Operand::Literal(0x1234_5678)
        );
    }

    /// A literal code with no value supplied is `Unrecognised`, not zero.
    #[test]
    fn a_literal_with_no_value_supplied_is_reported_rather_than_zeroed() {
        assert!(matches!(
            table().classify(255, None),
            Operand::Unrecognised(255)
        ));
    }

    /// A code in a table gap is `Unrecognised`.
    #[test]
    fn an_unmapped_code_is_reported_rather_than_guessed() {
        // 220 falls in a gap between the negative constants and the inline floats.
        assert!(matches!(
            table().classify(220, None),
            Operand::Unrecognised(220)
        ));
    }

    /// Overlapping ranges fail to load.
    #[test]
    fn overlapping_ranges_are_refused() {
        let result = OperandTable::load(
            r#"
            [[operand_code]]
            first = 0
            last = 100
            kind = "sgpr"

            [[operand_code]]
            first = 50
            last = 150
            kind = "vgpr"
            "#,
        );
        assert!(result.is_err(), "overlap must be refused");
    }

    /// A named range with no name fails to load.
    #[test]
    fn a_named_range_without_a_name_is_refused() {
        let result = OperandTable::load(
            r#"
            [[operand_code]]
            first = 106
            last = 106
            kind = "named"
            "#,
        );
        assert!(result.is_err());
    }

    /// Operands render as the reference disassembler prints them.
    #[test]
    fn operands_render_the_way_the_reference_prints_them() {
        assert_eq!(Operand::Scalar(3).to_string(), "s3");
        assert_eq!(Operand::Vector(11).to_string(), "v11");
        assert_eq!(Operand::Integer(-4).to_string(), "-4");
        assert_eq!(Operand::Named("vcc_lo".to_owned()).to_string(), "vcc_lo");
    }
}
