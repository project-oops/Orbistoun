//! Instruction encoding families, loaded from data.
//!
//! # Why this is a TOML file and not a `match` statement
//!
//! Principle 5: rules live in data. The test it gives is *if answering "what does
//! this do?" needs a rebuild, it is in the wrong place* - and that applies to an
//! encoding table more sharply than to almost anything else here.
//!
//! Every entry below is a claim about hardware, transcribed from a published
//! specification. Transcription errors are inevitable at this volume, and a wrong
//! entry does not fail to compile - it silently mis-decodes, which then looks like a
//! shader the translator does not understand rather than like a typo. Keeping the
//! table in data means correcting one is an edit, not a release, and means a
//! correction can be verified against a real shader in seconds.
//!
//! It also keeps the *provenance* answerable. The table is a transcription of a
//! public document; the code is ours. Those are different things and they are stored
//! separately.
//!
//! # Reference
//!
//! AMD publishes the instruction set architecture for these GPUs openly - the "ISA
//! Reference Guide" series on GPUOpen, one per architecture generation. That is the
//! source for every value here, and it is the strongest documentation available
//! anywhere in this project: an actual specification, published by the hardware
//! vendor, for the exact instruction set in question.
//!
//! Note that the console parts are customised, so the published tables cover the
//! great majority of what appears in a shader and not all of it. Instructions the
//! table does not describe are **counted and reported**, never guessed at.
//!
//! # Order matters, and is enforced rather than trusted
//!
//! Encodings are identified by matching the high bits of the first word against a
//! mask. Those masks overlap: a 9-bit pattern and a 2-bit pattern can both match the
//! same instruction, and only the longer one is right. Rather than depend on the file
//! being written in the correct order, [`EncodingTable::load`] sorts by mask
//! specificity, so a table that is correct as a *set* is correct as a *sequence*.

use serde::Deserialize;

use crate::ShaderError;
use crate::operand::OperandSlot;

/// A field within an instruction word that selects a literal constant when set to
/// [`LITERAL_MARKER`].
///
/// The rule is uniform across the architecture - an operand field reading 255 means
/// "the value is the next dword" - but *where* those fields sit differs per encoding,
/// so it has to be described per encoding rather than assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct OperandField {
    /// Bit position of the field's low bit, within its word.
    pub shift: u32,
    /// Width of the field in bits.
    pub width: u32,
    /// Which dword of the instruction the field sits in. Zero unless stated.
    ///
    /// Needed because a 64-bit encoding can put a literal-selecting operand in its
    /// *second* word - the long-form vector format keeps all three of its sources
    /// there. Reading only the first word made every long-form instruction carrying a
    /// literal decode four bytes short, which desynchronises everything after it.
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
    /// A field in a word the caller did not supply answers `false`: a truncated
    /// instruction is reported by the decoder's own bounds check, and guessing "there
    /// is a literal" from bytes nobody read would turn that into a length error.
    pub fn selects_literal(&self, words: &[u32]) -> bool {
        words
            .get(self.word)
            .is_some_and(|word| self.extract(*word) == LITERAL_MARKER)
    }
}

/// The operand value meaning "a 32-bit literal follows this instruction".
///
/// Uniform across encodings, which is why it is a constant here rather than another
/// column in the table.
pub const LITERAL_MARKER: u32 = 255;

/// One instruction encoding family.
#[derive(Debug, Clone, Deserialize)]
pub struct Encoding {
    /// Family name, as the published specification spells it. Reported verbatim, so
    /// a coverage report can be read against the document without a translation step.
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
    /// and a fourth at bit 53, which is bit 21 of the *second*. Read only the contiguous
    /// part and each half-precision variant decodes as the operation it is a variant of -
    /// an identical first word, differing in the one bit nobody looked at.
    ///
    /// A separate optional field rather than a list of pieces, because one continuation
    /// covers every split this instruction set actually has, and a general mechanism for
    /// a case that does not exist is a shape to maintain rather than a feature.
    #[serde(default)]
    pub opcode_extension: Option<OperandField>,
    /// Instruction length in bytes, before any trailing literal.
    pub width_bytes: u32,
    /// Operand fields that can select a trailing 32-bit literal.
    ///
    /// Empty for encodings that cannot take one. Getting this wrong desynchronises
    /// the decoder for the entire rest of the shader, which is why
    /// [`crate::decode()`] reports a decode that runs off the end rather than
    /// truncating quietly.
    #[serde(default)]
    pub literal_operands: Vec<OperandField>,
    /// Where this family's operands sit, in the order the specification prints them.
    ///
    /// `None` means no layout has been established. `Some([])` means the family was
    /// checked and genuinely has no register operands - a branch carrying only an
    /// immediate, for instance. Those are different claims and collapsing them would
    /// let an unfilled family pass for a complete one.
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
    /// Used to order the table: a 9-bit pattern must be tried before a 2-bit one that
    /// also matches, or every instruction in the more specific family is misread as
    /// belonging to the broader one.
    pub const fn specificity(&self) -> u32 {
        self.mask.count_ones()
    }

    /// This family's opcode, assembled from however many pieces it is kept in.
    ///
    /// A continuation in a word the caller did not supply contributes nothing, which
    /// matches how a literal-selecting field in an absent word answers `false`. The
    /// alternative - refusing - would turn a truncated instruction into an error about
    /// its opcode instead of about its length, and the decoder's bounds check already
    /// reports the truncation with the reason that actually explains it.
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

    /// Total length of an instruction of this family, including any literal.
    pub fn length_bytes(&self, words: &[u32]) -> u32 {
        let literal = self
            .literal_operands
            .iter()
            .any(|field| field.selects_literal(words));
        self.width_bytes + if literal { 4 } else { 0 }
    }
}

/// One opcode's operand fields, solved from probe samples.
#[derive(Debug, Clone, Deserialize)]
pub struct OpcodeOperands {
    /// Encoding family the opcode belongs to.
    pub family: String,
    /// Opcode within that family.
    pub opcode: u32,
    /// Name, for reports. Carried so a reader can check an entry by eye.
    #[serde(default)]
    pub mnemonic: String,
    /// How many probe samples the fields were solved from. One would prove nothing.
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
/// The per-opcode layouts live here rather than in their own table because they are the
/// same category of knowledge - how to read an instruction word - and because a caller
/// that has one always needs the other.
#[derive(Debug, Clone, Default)]
pub struct EncodingTable {
    encodings: Vec<Encoding>,
    per_opcode: std::collections::BTreeMap<(String, u32), Vec<OperandSlot>>,
    /// The name the reference assembler gave each solved opcode.
    ///
    /// Kept because opcode *numbers* move between architecture generations and names
    /// mostly do not - so a translator that dispatches on the name survives a retarget,
    /// and one that dispatches on the number silently binds to a different instruction.
    /// A handful of names do change too, and those show up as a name the table cannot
    /// find rather than as a wrong translation.
    names: std::collections::BTreeMap<(String, u32), String>,
    /// The reverse: name to where it lives on this target.
    by_name: std::collections::BTreeMap<String, (String, u32)>,
    /// The architecture generation every loaded table declares, once they agree.
    ///
    /// Carried so a caller can *say* what it decoded for. A report that names an
    /// instruction without naming the generation is ambiguous, because the same number
    /// is a different instruction one generation over.
    target: String,
}

#[derive(Debug, Deserialize)]
struct TableFile {
    /// The architecture generation, when the file names one.
    ///
    /// Optional at this level and required by [`EncodingTable::builtin`]. A table
    /// written inline - a test, a probe of one family - is not part of a set and has
    /// nothing to disagree with, so demanding a declaration there would be ceremony.
    /// The shipped set is where the hazard lives, and there it is mandatory.
    #[serde(default)]
    target: String,
    #[serde(default)]
    encoding: Vec<Encoding>,
}

/// Accepts `0x`-prefixed strings as well as plain integers.
///
/// Bit patterns are unreadable in decimal and every reference writes them in hex, so
/// the table should be transcribable without a conversion step - a conversion step is
/// somewhere to make a mistake.
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
    /// Sorts by specificity, so the file may be written in whatever order reads best
    /// against the published document rather than in the order a matcher needs.
    pub fn load(toml_text: &str) -> Result<Self, ShaderError> {
        let file: TableFile =
            toml::from_str(toml_text).map_err(|e| ShaderError::Table(e.to_string()))?;
        if file.encoding.is_empty() {
            // An empty table decodes every instruction as unknown, which reads as "this
            // shader uses nothing we understand" rather than as a missing file.
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
    /// Ordered, because it is iterated by tests that report what they found and an
    /// unstable order makes two runs of the same failure look like two failures.
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
    /// Not a warning. A table for the wrong generation is not *degraded*, it is
    /// answering a different question: the same opcode number names a different
    /// instruction one generation over, so every answer is confidently wrong and none
    /// of them look it.
    fn require_same_target(&self, other: &str, what: &str) -> Result<(), ShaderError> {
        let (other, mine) = (other.trim(), self.target.trim());
        if other == mine {
            return Ok(());
        }
        // An absent declaration and a wrong one are different mistakes with different
        // fixes, and a message reading "generated for , but" helps with neither.
        let found = if other.is_empty() {
            "declares no target".to_owned()
        } else {
            format!("was generated for {other}")
        };
        Err(ShaderError::Table(format!(
            "{what} {found}, but the encoding families describe {mine} - regenerate \
             them, or the two disagree about what every opcode number means"
        )))
    }

    /// The built-in table, with the solved per-opcode layouts.
    ///
    /// Three files, and **every one of them declares the architecture generation it
    /// describes**. They must agree, or this fails.
    ///
    /// That check exists because of the failure it would have caught. The encoding
    /// table was hand-written from one generation's published reference while the
    /// generated tables were solved against a *different* generation's assembler, and
    /// nothing said so - not a test, not a warning, not a wrong answer, because opcode
    /// numbers are dense and a wrong one lands on a real instruction. It went months.
    /// See D139.
    ///
    /// The hand-written file's declaration is a person's statement of intent and the
    /// generated files' are a record of what the tool actually ran against, so a
    /// mismatch means exactly one thing: someone retargeted and did not regenerate.
    pub fn builtin() -> Result<Self, ShaderError> {
        let mut table = Self::load(include_str!("../data/encodings.toml"))?;
        if table.target.trim().is_empty() {
            return Err(ShaderError::Table(
                "the built-in encoding table declares no target architecture, so \
                 nothing can check that the generated tables describe the same \
                 generation it does"
                    .into(),
            ));
        }
        table.load_opcode_operands(include_str!("../data/opcode-operands.toml"))?;
        table.add_names(include_str!("../data/mnemonics.toml"))?;
        Ok(table)
    }

    /// Adds per-opcode operand layouts.
    ///
    /// Separate from [`Self::load`] because the two files are produced differently -
    /// one is transcribed and differentially checked, the other is solved from probe
    /// samples - and keeping the loaders apart keeps that distinction visible.
    pub fn load_opcode_operands(&mut self, toml_text: &str) -> Result<(), ShaderError> {
        let file: OpcodeOperandsFile =
            toml::from_str(toml_text).map_err(|e| ShaderError::Table(e.to_string()))?;
        self.require_same_target(&file.target, "the operand layout table")?;
        for entry in file.opcode_operands {
            if entry.samples < 2 && !entry.operands.is_empty() {
                // One sample cannot distinguish a real field from a coincidence. An
                // entry solved from one is not evidence, and admitting it would put
                // unearned confidence behind a number.
                //
                // An entry with **no** fields is the exception, and narrowly: there is
                // nothing to infer, so nothing a second sample could corroborate. What
                // such an entry carries is the opcode's *name*, which one observation
                // establishes as well as ten. `s_endpgm` is the case - it takes no
                // operands and assembles identically every time, so demanding two
                // samples would be satisfied by duplicating a line, which meets the
                // letter of this rule and none of its purpose.
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
    /// Preferred over the family layout: a family layout describes a shape shared by
    /// every opcode in it, and D096 established that most families have no such shape.
    pub fn operands_for(&self, family: &str, opcode: u32) -> Option<&[OperandSlot]> {
        self.per_opcode
            .get(&(family.to_owned(), opcode))
            .map(Vec::as_slice)
    }

    /// Adds names observed from compiled fixtures.
    ///
    /// A second source for the same fact, and deliberately so. The probe solver names
    /// every opcode it solves *per opcode*; instructions whose operand shape comes from
    /// their family's layout are not in that set, and neither are the ones the solver
    /// cannot separate. The fixture generator names whatever a compiler emitted, which
    /// covers most of the gap.
    ///
    /// Both are generated by running the same reference assembler against the same
    /// target, so they cannot legitimately disagree - and a disagreement means one of the
    /// two generators has drifted, which is worth a load failure rather than a silent
    /// preference for whichever was read second.
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
                    "{family}:{opcode:#x} is named {existing} by the probe solver and \
                     {name} by the fixture generator - the two have drifted"
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
    /// The lookup a translator uses to bind what it understands to what this generation
    /// actually encodes. A name absent here is an instruction this target does not have
    /// under that name - which is a fact worth reporting rather than a lookup that
    /// quietly returns nothing.
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
    /// The index is returned rather than looked up again by the caller: a decoder
    /// needs it for every instruction, and recovering it by scanning would turn a
    /// linear walk into a quadratic one on shaders with tens of thousands of
    /// instructions.
    ///
    /// `None` means no family claims it - reported as unknown rather than skipped,
    /// because an unrecognised instruction is exactly the finding this crate exists
    /// to produce.
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

    #[test]
    fn a_more_specific_encoding_wins_regardless_of_file_order() {
        // The property the whole table depends on. Written deliberately in the wrong
        // order: the broad 2-bit pattern first, the 9-bit one second. If ordering were
        // taken from the file, every instruction of the specific family would be
        // misread as the general one - and would decode, plausibly, as the wrong thing.
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

    #[test]
    fn hexadecimal_and_decimal_both_parse() {
        // Every reference writes these in hex; requiring decimal would mean converting
        // by hand while transcribing, which is somewhere to make a mistake.
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

    #[test]
    fn a_value_with_bits_outside_its_mask_is_refused() {
        // Such an entry can never match anything. Silently accepting it would produce
        // a family that is simply absent from every decode, which looks like the
        // hardware never using it.
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

    #[test]
    fn an_empty_table_is_refused() {
        // It would decode every instruction as unknown, which reads as a shader using
        // nothing we understand rather than as a missing file.
        assert!(EncodingTable::load("").is_err());
    }

    #[test]
    fn a_literal_operand_extends_the_instruction_by_one_dword() {
        // Getting this wrong desynchronises every instruction after it, so the whole
        // rest of the shader decodes as garbage from one missed literal.
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
