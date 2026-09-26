//! Instruction encoding families, loaded from data.
//!
//! The table is data (D085): each entry is a hardware claim transcribed from the GPU
//! vendor's published ISA reference guide for the architecture generation, and a wrong
//! entry mis-decodes silently, so correcting one is an edit and not a release. The
//! hardware's GPU is customised, so instructions the table does not describe are counted
//! and reported, never guessed. Families match on masked high bits of the first word and
//! the masks overlap, so [`EncodingTable::load`] sorts by mask specificity: a table
//! correct as a set is correct as a sequence.

use serde::Deserialize;

use crate::ShaderError;
use crate::operand::OperandSlot;

/// A field within an instruction word that selects a literal constant when set to
/// [`LITERAL_MARKER`].
///
/// An operand field reading 255 means "the value is the next dword" across the
/// architecture, but where those fields sit differs per encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct OperandField {
    /// Bit position of the field's low bit, within its word.
    pub shift: u32,
    /// Width of the field in bits.
    pub width: u32,
    /// Which dword of the instruction the field sits in. Zero unless stated.
    ///
    /// A 64-bit encoding can put a literal-selecting operand in its second word: the
    /// long-form vector format keeps all three sources there.
    #[serde(default)]
    pub word: usize,
}

impl OperandField {
    /// Extracts this field from an instruction word.
    pub const fn extract(&self, word: u32) -> u32 {
        let mask = if self.width >= 32 {
            u32::MAX
        } else {
            (1u32 << self.width) - 1
        };
        (word >> self.shift) & mask
    }

    /// Whether this field selects a trailing literal constant.
    ///
    /// A field in a word the caller did not supply answers `false`: the decoder's own
    /// bounds check reports a truncated instruction.
    pub fn selects_literal(&self, words: &[u32]) -> bool {
        words
            .get(self.word)
            .is_some_and(|word| self.extract(*word) == LITERAL_MARKER)
    }
}

/// The operand value meaning "a 32-bit literal follows this instruction".
///
/// Uniform across encodings, so it is a constant rather than a table column.
pub const LITERAL_MARKER: u32 = 255;

/// One instruction encoding family.
#[derive(Debug, Clone, Deserialize)]
pub struct Encoding {
    /// Family name, as the published specification spells it.
    pub name: String,
    /// Bits of the first word that identify this family.
    #[serde(deserialize_with = "hex_u32")]
    pub mask: u32,
    /// Value those bits must take.
    #[serde(deserialize_with = "hex_u32")]
    pub value: u32,
    /// Position and width of the opcode field within the first word.
    pub opcode: OperandField,
    /// The rest of the opcode, when a family does not keep it in one piece.
    ///
    /// The typed-buffer family splits its opcode: three bits at 18:16 of the first word
    /// and a fourth at bit 53, bit 21 of the second. Reading only the contiguous part
    /// decodes each half-precision variant as the operation it varies. One continuation
    /// covers every split this instruction set has.
    #[serde(default)]
    pub opcode_extension: Option<OperandField>,
    /// Instruction length in bytes, before any trailing literal.
    pub width_bytes: u32,
    /// A field counting extra dwords this instruction carries beyond its fixed width.
    ///
    /// The image family may name its address registers individually, with the extra
    /// register numbers in dwords appended to the instruction; a miscount desynchronises
    /// the rest of the shader. [`None`] for every family whose length depends only on a
    /// literal.
    #[serde(default)]
    pub extra_dwords: Option<OperandField>,
    /// Operand fields that can select a trailing 32-bit literal.
    ///
    /// Empty for encodings that cannot take one. A miss desynchronises the rest of the
    /// shader, which [`crate::decode()`] reports as a decode running off the end.
    #[serde(default)]
    pub literal_operands: Vec<OperandField>,
    /// Where this family's operands sit, in the order the specification prints them.
    ///
    /// `None` means no layout is established. `Some([])` means the family has no register
    /// operands, such as a branch carrying only an immediate.
    #[serde(default)]
    pub operands: Option<Vec<OperandSlot>>,
}

impl Encoding {
    /// Whether `word` belongs to this family.
    pub const fn matches(&self, word: u32) -> bool {
        word & self.mask == self.value
    }

    /// How specific this encoding's match is, in bits.
    ///
    /// Orders the table: a 9-bit pattern is tried before a 2-bit one that also matches.
    pub const fn specificity(&self) -> u32 {
        self.mask.count_ones()
    }

    /// This family's opcode, assembled from however many pieces it is kept in.
    ///
    /// A continuation in a word the caller did not supply contributes nothing, so a
    /// truncated instruction is reported by the decoder's bounds check as a length error.
    pub fn opcode_of(&self, words: &[u32]) -> u32 {
        let low = words.first().map_or(0, |word| self.opcode.extract(*word));
        match &self.opcode_extension {
            Some(field) => match words.get(field.word) {
                Some(word) => low | (field.extract(*word) << self.opcode.width),
                None => low,
            },
            None => low,
        }
    }

    /// Total length of an instruction of this family, including any literal and any extra
    /// address dwords.
    pub fn length_bytes(&self, words: &[u32]) -> u32 {
        let literal = self
            .literal_operands
            .iter()
            .any(|field| field.selects_literal(words));
        let extra = self.extra_dwords.as_ref().map_or(0, |field| {
            words
                .get(field.word)
                .map_or(0, |word| field.extract(*word) * 4)
        });
        self.width_bytes + extra + if literal { 4 } else { 0 }
    }
}

/// One opcode's operand fields, solved from probe samples.
#[derive(Debug, Clone, Deserialize)]
pub struct OpcodeOperands {
    /// Encoding family the opcode belongs to.
    pub family: String,
    /// Opcode within that family.
    pub opcode: u32,
    /// Name, for reports.
    #[serde(default)]
    pub mnemonic: String,
    /// How many probe samples the fields were solved from.
    #[serde(default)]
    pub samples: u32,
    /// The fields, in the order the reference prints them.
    pub operands: Vec<OperandSlot>,
}

#[derive(Debug, Deserialize, Default)]
struct OpcodeOperandsFile {
    #[serde(default)]
    target: String,
    #[serde(default)]
    opcode_operands: Vec<OpcodeOperands>,
}

/// Every encoding family, ordered most-specific first, plus per-opcode operand layouts.
///
/// The per-opcode layouts live here because they describe how to read an instruction
/// word too, and a caller needing one needs the other.
#[derive(Debug, Clone, Default)]
pub struct EncodingTable {
    encodings: Vec<Encoding>,
    per_opcode: std::collections::BTreeMap<(String, u32), Vec<OperandSlot>>,
    /// The name the reference assembler gave each solved opcode.
    ///
    /// Opcode numbers move between architecture generations and names mostly do not, so
    /// a translator dispatching on names survives a retarget (D139).
    names: std::collections::BTreeMap<(String, u32), String>,
    /// The reverse: name to where it lives on this target.
    by_name: std::collections::BTreeMap<String, (String, u32)>,
    /// The architecture generation every loaded table declares, once they agree.
    ///
    /// The same opcode number is a different instruction one generation over, so a
    /// report names the generation it decoded for.
    target: String,
}

#[derive(Debug, Deserialize)]
struct TableFile {
    /// The architecture generation, when the file names one.
    ///
    /// Optional here and required by [`EncodingTable::builtin`]: an inline table in a test
    /// is not part of a set and has nothing to disagree with.
    #[serde(default)]
    target: String,
    #[serde(default)]
    encoding: Vec<Encoding>,
}

/// Accepts `0x`-prefixed strings as well as plain integers.
///
/// Bit patterns are written in hex in every reference, so the table transcribes without
/// a conversion step.
fn hex_u32<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    use serde::de::Error as _;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Int(u64),
        Text(String),
    }
    match Raw::deserialize(deserializer)? {
        Raw::Int(v) => u32::try_from(v).map_err(D::Error::custom),
        Raw::Text(s) => {
            let trimmed = s.trim();
            let (body, radix) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
                .map_or((trimmed, 10), |rest| (rest, 16));
            u32::from_str_radix(&body.replace('_', ""), radix).map_err(D::Error::custom)
        }
    }
}

impl EncodingTable {
    /// Parses a table from TOML.
    ///
    /// Sorts by specificity, so the file may follow the published document's order.
    pub fn load(toml_text: &str) -> Result<Self, ShaderError> {
        let file: TableFile =
            toml::from_str(toml_text).map_err(|e| ShaderError::Table(e.to_string()))?;
        if file.encoding.is_empty() {
            // An empty table would decode every instruction as unknown rather than
            // report a missing file.
            return Err(ShaderError::Table("the table declares no encodings".into()));
        }
        let mut encodings = file.encoding;
        for e in &encodings {
            if e.value & !e.mask != 0 {
                return Err(ShaderError::Table(format!(
                    "encoding {} has value {:#010x} with bits outside its mask {:#010x}",
                    e.name, e.value, e.mask
                )));
            }
            if e.width_bytes == 0 || e.width_bytes % 4 != 0 {
                return Err(ShaderError::Table(format!(
                    "encoding {} has width {} bytes; instructions are whole dwords",
                    e.name, e.width_bytes
                )));
            }
        }
        encodings.sort_by_key(|e| core::cmp::Reverse(e.specificity()));
        Ok(Self {
            encodings,
            per_opcode: std::collections::BTreeMap::new(),
            names: std::collections::BTreeMap::new(),
            by_name: std::collections::BTreeMap::new(),
            target: file.target,
        })
    }

    /// Every instruction name this target has, as (family, opcode, name).
    ///
    /// Ordered, so two runs of the same failure report identically.
    pub fn names(&self) -> impl Iterator<Item = (&str, u32, &str)> {
        self.names
            .iter()
            .map(|((family, opcode), name)| (family.as_str(), *opcode, name.as_str()))
    }

    /// The architecture generation these tables describe.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Refuses a table generated against a different architecture generation.
    ///
    /// The same opcode number names a different instruction one generation over, so a
    /// mismatched table answers every question wrongly.
    fn require_same_target(&self, other: &str, what: &str) -> Result<(), ShaderError> {
        let (other, mine) = (other.trim(), self.target.trim());
        if other == mine {
            return Ok(());
        }
        // An absent declaration and a wrong one have different fixes.
        let found = if other.is_empty() {
            "declares no target".to_owned()
        } else {
            format!("was generated for {other}")
        };
        Err(ShaderError::Table(format!(
            concat!(
                "{} {}, but the encoding families describe {} - regenerate ",
                "them, or the two disagree about what every opcode number means"
            ),
            what, found, mine
        )))
    }

    /// The built-in table, with the solved per-opcode layouts.
    ///
    /// Every one of the three files declares its architecture generation and they must
    /// agree (D139). Opcode numbers are dense, so a table from the wrong generation lands
    /// on real instructions and nothing else would notice; a mismatch means a retarget
    /// without regenerating.
    pub fn builtin() -> Result<Self, ShaderError> {
        let mut table = Self::load(include_str!("../data/encodings.toml"))?;
        if table.target.trim().is_empty() {
            return Err(ShaderError::Table(
                concat!(
                    "the built-in encoding table declares no target architecture, so ",
                    "nothing can check that the generated tables describe the same ",
                    "generation it does"
                )
                .into(),
            ));
        }
        table.load_opcode_operands(include_str!("../data/opcode-operands.toml"))?;
        table.add_names(include_str!("../data/mnemonics.toml"))?;
        Ok(table)
    }

    /// Adds per-opcode operand layouts.
    ///
    /// Separate from [`Self::load`] because this file is solved from probe samples, while
    /// the encoding table is transcribed.
    pub fn load_opcode_operands(&mut self, toml_text: &str) -> Result<(), ShaderError> {
        let file: OpcodeOperandsFile =
            toml::from_str(toml_text).map_err(|e| ShaderError::Table(e.to_string()))?;
        self.require_same_target(&file.target, "the operand layout table")?;
        for entry in file.opcode_operands {
            if entry.samples < 2 && !entry.operands.is_empty() {
                // One sample cannot distinguish a real field from a coincidence. An
                // entry with no fields is exempt: it carries only the opcode's name
                // (`s_endpgm`, for one), which one observation establishes.
                return Err(ShaderError::Table(format!(
                    "{}:{:#x} was solved from {} sample(s); at least two are needed",
                    entry.family, entry.opcode, entry.samples
                )));
            }
            self.names
                .insert((entry.family.clone(), entry.opcode), entry.mnemonic.clone());
            self.by_name
                .insert(entry.mnemonic, (entry.family.clone(), entry.opcode));
            self.per_opcode
                .insert((entry.family, entry.opcode), entry.operands);
        }
        Ok(())
    }

    /// Operand fields for a specific opcode, if they have been solved.
    ///
    /// Preferred over the family layout, since most families have no shape shared by
    /// every opcode (D097).
    pub fn operands_for(&self, family: &str, opcode: u32) -> Option<&[OperandSlot]> {
        self.per_opcode
            .get(&(family.to_owned(), opcode))
            .map(Vec::as_slice)
    }

    /// Adds names observed from compiled fixtures.
    ///
    /// The probe solver names only the opcodes it solves per opcode; the fixture generator
    /// names whatever a compiler emitted, which covers most of the rest. Both run the same
    /// reference assembler against the same target, so a disagreement means a generator
    /// has drifted.
    ///
    /// # Errors
    ///
    /// If the two sources name the same opcode differently.
    pub fn add_names(&mut self, toml_text: &str) -> Result<(), ShaderError> {
        let table = crate::MnemonicTable::load(toml_text)?;
        self.require_same_target(table.target(), "the instruction name table")?;
        for (family, opcode, name) in table.entries() {
            let key = (family.to_owned(), opcode);
            if let Some(existing) = self.names.get(&key)
                && existing != name
            {
                return Err(ShaderError::Table(format!(
                    concat!(
                        "{}:{:#x} is named {} by the probe solver and ",
                        "{} by the fixture generator - the two have drifted"
                    ),
                    family, opcode, existing, name
                )));
            }
            self.names.insert(key, name.to_owned());
            self.by_name
                .entry(name.to_owned())
                .or_insert_with(|| (family.to_owned(), opcode));
        }
        Ok(())
    }

    /// The name of an opcode on this target, if it has been probed.
    pub fn mnemonic_for(&self, family: &str, opcode: u32) -> Option<&str> {
        self.names
            .get(&(family.to_owned(), opcode))
            .map(String::as_str)
    }

    /// Where an instruction lives on this target, by name.
    ///
    /// A name absent here is an instruction this target does not have under that name.
    pub fn find_by_name(&self, mnemonic: &str) -> Option<(&str, u32)> {
        self.by_name
            .get(mnemonic)
            .map(|(family, opcode)| (family.as_str(), *opcode))
    }

    /// How many opcodes have solved operand layouts.
    pub fn solved_opcode_count(&self) -> usize {
        self.per_opcode.len()
    }

    /// Finds the encoding for an instruction word, with its index.
    ///
    /// The index is returned so the decoder need not rescan for it per instruction.
    /// `None` means no family claims the word, reported as unknown.
    pub fn lookup(&self, word: u32) -> Option<(u16, &Encoding)> {
        self.encodings
            .iter()
            .enumerate()
            .find(|(_, e)| e.matches(word))
            .and_then(|(i, e)| u16::try_from(i).ok().map(|i| (i, e)))
    }

    /// Every encoding, most specific first.
    pub fn encodings(&self) -> &[Encoding] {
        &self.encodings
    }
}

#[cfg(test)]
mod tests {
    use super::{Encoding, EncodingTable, OperandField};

    /// The more specific encoding wins whatever the file order.
    #[test]
    fn a_more_specific_encoding_wins_regardless_of_file_order() {
        // Written in the wrong order: the broad 2-bit pattern first.
        let table = EncodingTable::load(
            r#"
            [[encoding]]
            name = "BROAD"
            mask = "0xC0000000"
            value = "0x80000000"
            opcode = { shift = 23, width = 7 }
            width_bytes = 4

            [[encoding]]
            name = "SPECIFIC"
            mask = "0xFF800000"
            value = "0xBF800000"
            opcode = { shift = 16, width = 7 }
            width_bytes = 4
            "#,
        )
        .expect("table");

        assert_eq!(
            table.encodings()[0].name,
            "SPECIFIC",
            "sorted by specificity"
        );
        let word = 0xBF80_0000;
        assert_eq!(table.lookup(word).expect("matched").1.name, "SPECIFIC");
    }

    /// Hexadecimal strings parse as table values.
    #[test]
    fn hexadecimal_and_decimal_both_parse() {
        let table = EncodingTable::load(
            r#"
            [[encoding]]
            name = "HEX"
            mask = "0xFF800000"
            value = "0xBF800000"
            opcode = { shift = 16, width = 7 }
            width_bytes = 4
            "#,
        )
        .expect("table");
        assert_eq!(table.encodings()[0].mask, 0xFF80_0000);
    }

    /// A value with bits outside its mask, which could never match, is refused.
    #[test]
    fn a_value_with_bits_outside_its_mask_is_refused() {
        let result = EncodingTable::load(
            r#"
            [[encoding]]
            name = "IMPOSSIBLE"
            mask = "0xFF800000"
            value = "0xBF800001"
            opcode = { shift = 16, width = 7 }
            width_bytes = 4
            "#,
        );
        assert!(result.is_err(), "unmatchable entry must be refused");
    }

    /// A width that is not whole dwords is refused.
    #[test]
    fn a_width_that_is_not_whole_dwords_is_refused() {
        let result = EncodingTable::load(
            r#"
            [[encoding]]
            name = "ODD"
            mask = "0xFF800000"
            value = "0xBF800000"
            opcode = { shift = 16, width = 7 }
            width_bytes = 6
            "#,
        );
        assert!(result.is_err());
    }

    /// An empty table is refused rather than decoding everything as unknown.
    #[test]
    fn an_empty_table_is_refused() {
        assert!(EncodingTable::load("").is_err());
    }

    /// Operand 255 extends the instruction by one literal dword.
    #[test]
    fn a_literal_operand_extends_the_instruction_by_one_dword() {
        let encoding = Encoding {
            name: "WITH_LITERAL".into(),
            mask: 0x8000_0000,
            value: 0x0000_0000,
            opcode: OperandField {
                shift: 25,
                width: 6,
                word: 0,
            },
            opcode_extension: None,
            width_bytes: 4,
            extra_dwords: None,
            literal_operands: vec![OperandField {
                shift: 0,
                width: 9,
                word: 0,
            }],
            operands: None,
        };
        assert_eq!(encoding.length_bytes(&[0x0000_0000]), 4, "no literal");
        assert_eq!(
            encoding.length_bytes(&[0x0000_00FF]),
            8,
            "operand 255 pulls in a trailing dword"
        );
    }

    /// The built-in table loads, ordered most-specific first.
    #[test]
    fn the_builtin_table_loads_and_is_ordered() {
        let table = EncodingTable::builtin().expect("the built-in table must parse");
        assert!(!table.encodings().is_empty());
        let specificities: Vec<u32> = table
            .encodings()
            .iter()
            .map(Encoding::specificity)
            .collect();
        let mut sorted = specificities.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(specificities, sorted, "must be most-specific first");
    }
}
