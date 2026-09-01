//! Walking a shader binary into instructions.
//!
//! # The unknown-length problem
//!
//! Decoding a variable-length instruction stream needs each instruction's length to
//! find the next one. That length comes from the instruction's encoding - so an
//! instruction whose encoding is not recognised has **no known length**, and the
//! decoder cannot reliably find where the next one begins.
//!
//! There is no clever way out of that. The decoder advances by the minimum
//! instruction size and marks the position desynchronised, because the alternatives
//! are worse: stopping loses every instruction after the first gap, and guessing
//! produces a long tail of confidently decoded nonsense that reads exactly like real
//! data.
//!
//! What it does instead is **report it**. [`Decode::desynchronised`] means everything
//! after the first unknown is suspect, and a coverage number from such a decode should
//! be read as a lower bound rather than a measurement. That distinction is the whole
//! point - a tool that cannot tell you when to distrust it is worse than no tool.
//!
//! # Running off the end is a signal, not an error
//!
//! If the final instruction claims to extend past the end of the buffer, the length
//! calculation is wrong somewhere earlier - most likely a missed trailing literal,
//! which shifts every subsequent instruction by one dword. That is reported too, and
//! it is the single most useful indicator that the encoding table has a mistake in it.

use crate::encoding::EncodingTable;
use crate::operand::{Operand, OperandTable, SlotKind};

/// The smallest an instruction can be, and therefore how far the decoder advances
/// when it does not know how far to advance.
pub const MIN_INSTRUCTION_BYTES: u32 = 4;

/// One decoded instruction.
///
/// No longer `Copy`: operands carry names, and a name is a `String`. That costs an
/// allocation per named operand, which is worth it here - the alternative is interning
/// or a fixed enum of every special register, and both trade a real cost in clarity
/// for a saving on a path that runs once per instruction rather than per frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    /// Byte offset within the shader binary.
    pub offset: u32,
    /// Total length in bytes, including any trailing literal.
    pub length: u32,
    /// The first word, kept so a report can quote what was actually seen.
    pub word: u32,
    /// The second word of the instruction's fixed part, for encodings that have one.
    ///
    /// Not an operand and not a literal - the fixed second dword of an eight-byte
    /// encoding. Present because some fields are neither operands nor opcode: the
    /// vector ALU's long form carries per-source negate and absolute flags, and a
    /// translator that could not see them would silently compute `a + b` where the
    /// guest wrote `a + -b`.
    ///
    /// Surfaced as the raw word rather than as decoded flags because which bits mean
    /// what is a property of the sub-encoding, and the decoder does not model those.
    pub second_word: Option<u32>,
    /// Index into [`EncodingTable::encodings`], or `None` if unrecognised.
    pub encoding: Option<u16>,
    /// Opcode within its family. Meaningless when `encoding` is `None`.
    pub opcode: u32,
    /// What it operates on, in the order the specification prints them.
    ///
    /// Empty when the family's operand layout has not been established yet, which is
    /// **not** the same as an instruction with no operands - see
    /// [`Instruction::operands_decoded`].
    pub operands: Vec<Operand>,
    /// Whether the encoding declared an operand layout at all.
    ///
    /// True with an empty `operands` means the family was checked and genuinely takes
    /// no register operands. False means nobody has taught the decoder this family
    /// yet. Separating those is what stops the second reading as the first.
    pub operands_decoded: bool,
}

impl Instruction {
    /// Whether the decoder recognised this instruction's family.
    pub const fn is_known(&self) -> bool {
        self.encoding.is_some()
    }
}

/// The result of walking one shader.
#[derive(Debug, Clone, Default)]
pub struct Decode {
    /// Every instruction, in order.
    pub instructions: Vec<Instruction>,
    /// Set once an unrecognised instruction has been passed.
    ///
    /// Everything after that point is suspect: the decoder had to guess where the
    /// next instruction began. Coverage from a desynchronised decode is a lower
    /// bound.
    pub desynchronised: bool,
    /// The last instruction claimed to extend past the end of the buffer.
    ///
    /// Strong evidence of a length error earlier in the stream - usually a missed
    /// trailing literal - and therefore the best single signal that the encoding
    /// table needs correcting.
    pub overran: bool,
    /// Bytes left over that were not a whole dword.
    ///
    /// A shader binary should be whole dwords; a remainder means the length passed in
    /// was wrong, or the buffer is not what it was thought to be.
    pub trailing_bytes: usize,
    /// Whether the instruction that ends a program was reached.
    ///
    /// Only interesting for a shader with no declared length. Its absence there means
    /// the address was wrong or the window was too small; both are errors, and neither
    /// is "a shader that runs to the end of memory".
    pub terminated: bool,
    /// Bytes consumed.
    ///
    /// For [`decode_program`] this is the shader's actual length, which is what a cache
    /// key should be computed over - the window it was read from is arbitrary.
    pub consumed: usize,
}

impl Decode {
    /// How many instructions were recognised.
    pub fn known(&self) -> usize {
        self.instructions.iter().filter(|i| i.is_known()).count()
    }

    /// How many were not.
    pub fn unknown(&self) -> usize {
        self.instructions.len() - self.known()
    }

    /// Fraction of instructions recognised, in `0.0..=1.0`.
    ///
    /// An empty shader scores 1.0 rather than 0.0: nothing was not understood. That
    /// avoids an empty or failed capture dragging a corpus average down while looking
    /// like a translation problem.
    pub fn coverage(&self) -> f64 {
        if self.instructions.is_empty() {
            return 1.0;
        }
        // Converted through u32, which is lossless into f64. A shader with more than
        // four billion instructions is not a shader.
        let known = u32::try_from(self.known()).unwrap_or(u32::MAX);
        let total = u32::try_from(self.instructions.len()).unwrap_or(u32::MAX);
        f64::from(known) / f64::from(total)
    }

    /// Whether this decode can be trusted as a measurement rather than a lower bound.
    pub const fn is_trustworthy(&self) -> bool {
        !self.desynchronised && !self.overran && self.trailing_bytes == 0
    }
}

/// Walks `bytes` as an instruction stream.
///
/// Never fails: a shader that cannot be decoded is a *finding*, reported through the
/// flags on [`Decode`], not an error to propagate. Returning `Err` here would mean a
/// corpus sweep stopping at the first strange binary instead of telling you how many
/// strange binaries there are.
pub fn decode(bytes: &[u8], table: &EncodingTable, operands: &OperandTable) -> Decode {
    decode_inner(bytes, table, operands, false)
}

/// Decodes a shader that has no declared length, stopping where the program ends.
///
/// # Why this is a different function
///
/// [`decode`] decodes the slice it is given, which is right when the slice *is* the
/// shader - a fixture, a dumped binary, anything with a known extent. A shader read out
/// of guest memory has no extent. It begins at an address a register named and it ends
/// at the instruction that ends a program, and everything after that is whatever the
/// guest happened to put there.
///
/// Decoding past it is not merely wasteful. The bytes after a shader are usually not
/// instructions, so they desynchronise the decode - and `is_trustworthy` would then
/// report a perfectly good shader as untrustworthy because of data that is not part of
/// it.
///
/// [`Decode::terminated`] says whether the end was actually found. It not being found
/// means the address was wrong or the window was too small, and both are errors rather
/// than a shader that happens to run to the end.
pub fn decode_program(bytes: &[u8], table: &EncodingTable, operands: &OperandTable) -> Decode {
    decode_inner(bytes, table, operands, true)
}

fn decode_inner(
    bytes: &[u8],
    table: &EncodingTable,
    operands: &OperandTable,
    stop_at_end: bool,
) -> Decode {
    let mut result = Decode {
        trailing_bytes: bytes.len() % 4,
        ..Decode::default()
    };

    let mut offset: usize = 0;
    while offset + 4 <= bytes.len() {
        // Little-endian, like every other integer in this format.
        let word = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]);

        let Recognised {
            length,
            encoding,
            opcode,
            operands: decoded_operands,
            has_layout,
            second_word,
        } = recognise(bytes, offset, word, table, operands, &mut result);

        if offset + length as usize > bytes.len() {
            // The instruction claims to extend past the buffer. Record it and stop:
            // continuing would read whatever follows in memory as instructions.
            result.overran = true;
            result.instructions.push(Instruction {
                offset: u32::try_from(offset).unwrap_or(u32::MAX),
                length,
                word,
                second_word,
                encoding,
                opcode,
                operands: decoded_operands,
                operands_decoded: has_layout,
            });
            break;
        }

        let named = encoding
            .and_then(|index| table.encodings().get(usize::from(index)))
            .and_then(|found| table.mnemonic_for(&found.name, opcode));
        let ends_the_program = named == Some(PROGRAM_END);
        let is_padding = named == Some(PADDING);

        result.instructions.push(Instruction {
            offset: u32::try_from(offset).unwrap_or(u32::MAX),
            length,
            word,
            second_word,
            encoding,
            opcode,
            operands: decoded_operands,
            operands_decoded: has_layout,
        });
        offset += length as usize;

        // Padding is not code, and the reference is explicit that it is "treated as an
        // illegal instruction, used to pad past the end of shaders" - it exists so an
        // instruction prefetch running off the end faults instead of executing whatever
        // follows. So it ends the decode in **both** modes, and reaching it is not a
        // desynchronised decode; it is the end.
        //
        // This is a different question from `ends_the_program`, and conflating them cost
        // a real failure. A shader with two exits ends its wave twice, and stopping at
        // the first `s_endpgm` truncated a compiled shader in the middle - a branch then
        // targeted an instruction that was no longer decoded, and the report blamed the
        // branch.
        if is_padding {
            result.instructions.pop();
            result.consumed = offset - length as usize;
            result.trailing_bytes = 0;
            return result;
        }

        if ends_the_program {
            result.terminated = true;
            if stop_at_end {
                // Whatever follows is not part of this shader. `trailing_bytes` is
                // recomputed against what was actually consumed, so a window larger than
                // the shader does not read as a malformed one.
                result.trailing_bytes = 0;
                result.consumed = offset;
                return result;
            }
        }
    }

    result.consumed = offset;
    result
}

/// The instruction that ends a program.
///
/// Named here rather than taken from the translator's supported list, because a decoder
/// that needed the translator to tell it where a shader ends would be the wrong way
/// round - decoding is the layer below.
///
/// A **name**, not a family and an opcode number. It was `SOPP` opcode 1, which is what
/// this instruction happens to be on two architecture generations running and is not a
/// property anything guarantees (D139).
const PROGRAM_END: &str = "s_endpgm";

/// The instruction compilers pad past the end of a shader with.
///
/// Deliberately an illegal instruction: it is there so that an instruction prefetch
/// running past the end of a shader raises an interrupt rather than executing whatever
/// is in memory afterwards. Fifty of them follow a compiled shader in this project's own
/// fixtures.
///
/// It therefore ends the decode wherever it appears. Everything after a shader is not
/// the shader, and here the hardware says so rather than leaving it to be inferred.
const PADDING: &str = "s_code_end";

/// One instruction, as far as the tables can describe it.
struct Recognised {
    length: u32,
    encoding: Option<u16>,
    opcode: u32,
    operands: Vec<Operand>,
    has_layout: bool,
    second_word: Option<u32>,
}

/// Looks one instruction up and decodes its operands.
///
/// Extracted from the walk because the two do different jobs: the walk decides where
/// instructions begin and this decides what one is. Keeping them together made a
/// function long enough that neither was easy to follow.
fn recognise(
    bytes: &[u8],
    offset: usize,
    word: u32,
    table: &EncodingTable,
    operands: &OperandTable,
    result: &mut Decode,
) -> Recognised {
    if let Some((index, found)) = table.lookup(word) {
        // Both words, because a 64-bit encoding can select a literal from its second
        // one and reading only the first understates the length by four bytes.
        let second = read_word(bytes, offset + 4);
        let words: Vec<u32> = core::iter::once(word).chain(second).collect();
        let length = found.length_bytes(&words);
        // A literal lives in the dword after the instruction's fixed part. It
        // is read here rather than by the operand table, because only the
        // encoding knows where the fixed part ends.
        let literal = if length > found.width_bytes {
            read_word(bytes, offset + found.width_bytes as usize)
        } else {
            None
        };
        // Per-opcode first: a family layout only describes a family whose
        // shape is fixed, and D096 established that most are not. Falling back
        // to the family is right for the ones that are.
        let slots: Option<&[crate::operand::OperandSlot]> = table
            .operands_for(&found.name, found.opcode_of(&words))
            .or(found.operands.as_deref());
        let decoded = slots.map_or_else(Vec::new, |slots| {
            slots
                .iter()
                .map(|slot| {
                    // An implicit operand reads no bits at all, so it is settled
                    // before any word is fetched - and it stays correct for an
                    // instruction whose later dwords are out of range.
                    if slot.kind == SlotKind::Implicit {
                        return slot
                            .implicit
                            .as_ref()
                            .map_or(Operand::Unrecognised(u16::MAX), |name| {
                                Operand::Named(name.clone())
                            });
                    }
                    // A slot may name a later dword. Reading past the instruction's
                    // fixed part would pick up a trailing literal or the start of
                    // the next instruction, so an out-of-range word yields nothing
                    // rather than whatever happens to sit there.
                    let source_word = if slot.word == 0 {
                        Some(word)
                    } else if slot.word * 4 < found.width_bytes {
                        read_word(bytes, offset + (slot.word as usize) * 4)
                    } else {
                        None
                    };
                    source_word.map_or(Operand::Unrecognised(u16::MAX), |w| {
                        let code = slot.extract(w);
                        match slot.kind {
                            SlotKind::Source => operands.classify(code, literal),
                            SlotKind::Vgpr => {
                                Operand::Vector(u16::try_from(code).unwrap_or(u16::MAX))
                            }
                            SlotKind::Immediate => Operand::Immediate(i64::from(code)),
                            // Handled above, before a word was read.
                            SlotKind::Implicit => Operand::Unrecognised(u16::MAX),
                        }
                    })
                })
                .collect()
        });
        Recognised {
            length,
            encoding: Some(index),
            opcode: found.opcode_of(&words),
            operands: decoded,
            has_layout: slots.is_some(),
            second_word: (found.width_bytes >= 8)
                .then(|| read_word(bytes, offset + 4))
                .flatten(),
        }
    } else {
        // Length unknown. Advance minimally and say so - see the module note.
        result.desynchronised = true;
        Recognised {
            length: MIN_INSTRUCTION_BYTES,
            encoding: None,
            opcode: 0,
            operands: Vec::new(),
            has_layout: false,
            second_word: None,
        }
    }
}

/// Reads a little-endian word, or `None` if it would run past the buffer.
fn read_word(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
mod tests {
    /// The built-in operand table. Every decode needs one now that operands are read.
    fn operands() -> crate::operand::OperandTable {
        crate::operand::OperandTable::builtin().expect("built-in operand table")
    }

    use super::{MIN_INSTRUCTION_BYTES, decode};
    use crate::encoding::EncodingTable;

    /// A table with two families, one fixed-length and one that can take a literal.
    /// **Generated, never extracted** - the same rule the rest of the project follows
    /// for test material.
    fn table() -> EncodingTable {
        EncodingTable::load(
            r#"
            [[encoding]]
            name = "FIXED8"
            mask = "0xFC000000"
            value = "0xD0000000"
            opcode = { shift = 16, width = 10 }
            width_bytes = 8

            [[encoding]]
            name = "MAYBE_LITERAL"
            mask = "0xFE000000"
            value = "0x7E000000"
            opcode = { shift = 9, width = 8 }
            width_bytes = 4
            literal_operands = [{ shift = 0, width = 9 }]
            "#,
        )
        .expect("table")
    }

    fn stream(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn instructions_are_walked_in_order_at_their_own_lengths() {
        // Three instructions of two different lengths. If lengths were assumed
        // uniform, the offsets would drift and every later instruction would be read
        // from the middle of its predecessor.
        let bytes = stream(&[
            0x7E00_0000, // 4 bytes, no literal
            0xD000_0000,
            0x0000_0000, // 8 bytes
            0x7E00_0000, // 4 bytes
        ]);
        let decoded = decode(&bytes, &table(), &operands());
        assert_eq!(decoded.instructions.len(), 3);
        assert_eq!(decoded.instructions[0].offset, 0);
        assert_eq!(decoded.instructions[1].offset, 4);
        assert_eq!(decoded.instructions[1].length, 8);
        assert_eq!(decoded.instructions[2].offset, 12);
        assert!(decoded.is_trustworthy());
    }

    #[test]
    fn a_trailing_literal_is_consumed_rather_than_decoded_as_an_instruction() {
        // The literal is data, not code. Decoding it as an instruction would both
        // invent an instruction and shift everything after it.
        let bytes = stream(&[
            0x7E00_00FF, // operand 255: a literal follows
            0xDEAD_BEEF, // the literal
            0x7E00_0000, // the next real instruction
        ]);
        let decoded = decode(&bytes, &table(), &operands());
        assert_eq!(
            decoded.instructions.len(),
            2,
            "the literal is not an instruction"
        );
        assert_eq!(decoded.instructions[0].length, 8);
        assert_eq!(decoded.instructions[1].offset, 8);
        assert!(decoded.is_trustworthy());
    }

    #[test]
    fn an_unrecognised_instruction_marks_the_decode_desynchronised() {
        // The decoder does not know how long it is, so it cannot know where the next
        // one starts. Saying so is the whole point; a coverage number from here on is
        // a lower bound.
        let bytes = stream(&[0xFFFF_FFFF, 0x7E00_0000]);
        let decoded = decode(&bytes, &table(), &operands());
        assert!(decoded.desynchronised);
        assert!(!decoded.is_trustworthy());
        assert_eq!(decoded.unknown(), 1);
        assert_eq!(
            decoded.instructions[1].offset, MIN_INSTRUCTION_BYTES,
            "advances minimally past the unknown"
        );
    }

    #[test]
    fn an_instruction_extending_past_the_buffer_is_reported_not_read() {
        // Continuing would read whatever happens to follow in memory as instructions.
        // This flag is also the best single indicator that a length in the encoding
        // table is wrong.
        let bytes = stream(&[0xD000_0000]); // claims 8 bytes, only 4 present
        let decoded = decode(&bytes, &table(), &operands());
        assert!(decoded.overran);
        assert!(!decoded.is_trustworthy());
    }

    #[test]
    fn a_buffer_that_is_not_whole_dwords_is_reported() {
        let mut bytes = stream(&[0x7E00_0000]);
        bytes.push(0xAB);
        let decoded = decode(&bytes, &table(), &operands());
        assert_eq!(decoded.trailing_bytes, 1);
        assert!(!decoded.is_trustworthy());
    }

    #[test]
    fn coverage_is_the_recognised_fraction() {
        let bytes = stream(&[0x7E00_0000, 0xFFFF_FFFF, 0x7E00_0000, 0x7E00_0000]);
        let decoded = decode(&bytes, &table(), &operands());
        assert_eq!(decoded.known(), 3);
        assert_eq!(decoded.unknown(), 1);
        assert!((decoded.coverage() - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn an_empty_shader_scores_full_coverage_rather_than_zero() {
        // Nothing was not understood. Scoring zero would let an empty or failed
        // capture drag a corpus average down while looking like a translation problem.
        let decoded = decode(&[], &table(), &operands());
        assert!((decoded.coverage() - 1.0).abs() < f64::EPSILON);
        assert!(decoded.is_trustworthy());
    }

    #[test]
    fn the_builtin_table_decodes_a_generated_stream_without_desynchronising() {
        // Not a claim that the table is correct - only that it is self-consistent
        // enough to walk a stream built from its own declared encodings.
        //
        // The stream is **built from the table**, one instruction per family, rather
        // than from a handful of written-down words. The written-down version passed for
        // months and then failed the moment the target generation changed, because one
        // of its four constants was the previous generation's long-form vector value and
        // matched nothing - which is a fact about the test, not about the table. A test
        // that hard-codes what it is testing stops testing it exactly when it matters.
        let builtin = EncodingTable::builtin().expect("builtin");
        let words: Vec<u32> = builtin
            .encodings()
            .iter()
            .flat_map(|encoding| {
                // The bare identifying value: opcode zero, every operand field zero, so
                // no field reads 255 and no trailing literal is implied.
                let trailing = (encoding.width_bytes / 4).saturating_sub(1) as usize;
                core::iter::once(encoding.value).chain(core::iter::repeat_n(0, trailing))
            })
            .collect();

        let decoded = decode(&stream(&words), &builtin, &operands());
        assert!(!decoded.desynchronised, "builtin table left a gap");
        assert!(!decoded.overran);
        assert_eq!(
            decoded.known(),
            builtin.encodings().len(),
            "every declared family should recognise its own identifying value"
        );
    }
}
