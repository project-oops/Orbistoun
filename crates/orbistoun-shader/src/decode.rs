//! Walking a shader binary into instructions.
//!
//! An instruction's length comes from its encoding, so an unrecognised instruction has no
//! known length. The decoder advances by the minimum instruction size and sets
//! [`Decode::desynchronised`]: everything after the first unknown is suspect and coverage
//! is a lower bound. Stopping would lose the rest; guessing would produce plausible
//! nonsense. A final instruction running past the buffer (usually a missed trailing
//! literal) is reported as [`Decode::overran`], the best single sign of an encoding
//! table fault.

use crate::encoding::EncodingTable;
use crate::operand::{Operand, OperandTable, SlotKind};

/// The smallest an instruction can be, and how far the decoder advances past an
/// unrecognised one.
pub const MIN_INSTRUCTION_BYTES: u32 = 4;

/// One decoded instruction.
///
/// Operands carry names as `String`s; the allocation per named operand runs once per
/// instruction, not per frame.
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
    /// The fixed second dword of an eight-byte encoding, which holds fields that are
    /// neither operands nor opcode: the vector ALU's long form carries per-source negate
    /// and absolute flags here. Surfaced raw because which bits mean what is a property of
    /// the sub-encoding, which the decoder does not model.
    pub second_word: Option<u32>,
    /// Index into [`EncodingTable::encodings`], or `None` if unrecognised.
    pub encoding: Option<u16>,
    /// Opcode within its family. Meaningless when `encoding` is `None`.
    pub opcode: u32,
    /// What it operates on, in the order the specification prints them.
    ///
    /// Empty when the family's operand layout is not established, which differs from an
    /// instruction with no operands - see [`Instruction::operands_decoded`].
    pub operands: Vec<Operand>,
    /// Whether the encoding declared an operand layout at all.
    ///
    /// True with an empty `operands` means the family takes no register operands; false
    /// means the decoder has no layout for the family.
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
    /// Everything after that point is suspect: the decoder guessed where the next
    /// instruction began. Coverage from a desynchronised decode is a lower bound.
    pub desynchronised: bool,
    /// The last instruction claimed to extend past the end of the buffer.
    ///
    /// Evidence of a length error earlier in the stream, usually a missed trailing
    /// literal, and so of an encoding table fault.
    pub overran: bool,
    /// Bytes left over that were not a whole dword.
    ///
    /// A remainder means the length passed in was wrong, or the buffer is not a shader.
    pub trailing_bytes: usize,
    /// Whether the instruction that ends a program was reached.
    ///
    /// Meaningful for a shader with no declared length, where its absence means the
    /// address was wrong or the window too small.
    pub terminated: bool,
    /// Bytes consumed.
    ///
    /// For [`decode_program`] this is the shader's actual length, which a cache key is
    /// computed over; the window it was read from is arbitrary.
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
    /// An empty shader scores 1.0, so an empty or failed capture does not drag a corpus
    /// average down.
    pub fn coverage(&self) -> f64 {
        if self.instructions.is_empty() {
            return 1.0;
        }
        // Converted through u32, which is lossless into f64.
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
/// Never fails: an undecodable shader is a finding reported through the flags on
/// [`Decode`], so a corpus sweep counts every strange binary.
pub fn decode(bytes: &[u8], table: &EncodingTable, operands: &OperandTable) -> Decode {
    decode_inner(bytes, table, operands, false)
}

/// Decodes a shader that has no declared length, stopping where the program ends.
///
/// [`decode`] decodes the whole slice, which suits a shader with a known extent. A shader
/// read from guest memory begins at an address a register named and ends at the
/// instruction that ends a program; the bytes after it are usually not instructions and
/// would desynchronise the decode of a good shader. [`Decode::terminated`] says whether
/// the end was found; if not, the address was wrong or the window too small.
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
            // The instruction claims to extend past the buffer. Record it and stop
            // rather than read whatever follows as instructions.
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

        // Padding ends the decode in both modes: the reference describes it as "treated as
        // an illegal instruction, used to pad past the end of shaders", so a prefetch
        // running off the end faults. It differs from `ends_the_program`: a shader with
        // two exits contains `s_endpgm` twice and continues past the first.
        if is_padding {
            result.instructions.pop();
            result.consumed = offset - length as usize;
            result.trailing_bytes = 0;
            return result;
        }

        if ends_the_program {
            result.terminated = true;
            if stop_at_end {
                // Whatever follows is not part of this shader, so trailing bytes are
                // counted against what was consumed, not the window.
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
/// Named here rather than taken from the translator's supported list, because decoding is
/// the layer below translation. Keyed by name, not family and opcode number (D139).
const PROGRAM_END: &str = "s_endpgm";

/// The instruction compilers pad past the end of a shader with.
///
/// An illegal instruction, so a prefetch running past a shader's end raises an interrupt
/// rather than executing what follows. Compiled shaders are followed by a run of them,
/// so it ends the decode wherever it appears.
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
/// The walk decides where instructions begin; this decides what one is.
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
        // one; reading only the first understates the length by four bytes.
        let second = read_word(bytes, offset + 4);
        let words: Vec<u32> = core::iter::once(word).chain(second).collect();
        let length = found.length_bytes(&words);
        // A literal lives in the dword after the instruction's fixed part, which only
        // the encoding knows the end of.
        let literal = if length > found.width_bytes {
            read_word(bytes, offset + found.width_bytes as usize)
        } else {
            None
        };
        // Per-opcode layout first, since most families do not have a fixed shape (D097);
        // the family layout covers those that do.
        let slots: Option<&[crate::operand::OperandSlot]> = table
            .operands_for(&found.name, found.opcode_of(&words))
            .or(found.operands.as_deref());
        let decoded = slots.map_or_else(Vec::new, |slots| {
            slots
                .iter()
                .map(|slot| {
                    // An implicit operand reads no bits, so it is settled before any
                    // word is fetched.
                    if slot.kind == SlotKind::Implicit {
                        return slot
                            .implicit
                            .as_ref()
                            .map_or(Operand::Unrecognised(u16::MAX), |name| {
                                Operand::Named(name.clone())
                            });
                    }
                    // A slot may name a later dword. One beyond the fixed part would be
                    // a trailing literal or the next instruction, so it yields nothing.
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
        // Length unknown: advance minimally and mark the decode desynchronised.
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
    /// The built-in operand table, which every decode needs.
    fn operands() -> crate::operand::OperandTable {
        crate::operand::OperandTable::builtin().expect("built-in operand table")
    }

    use super::{MIN_INSTRUCTION_BYTES, decode};
    use crate::encoding::EncodingTable;

    /// A generated table with two families, one fixed-length and one that can take a
    /// literal.
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

    /// Instructions of different lengths are walked at their own offsets.
    #[test]
    fn instructions_are_walked_in_order_at_their_own_lengths() {
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

    /// A trailing literal is consumed as data, not decoded as an instruction.
    #[test]
    fn a_trailing_literal_is_consumed_rather_than_decoded_as_an_instruction() {
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

    /// An unrecognised instruction marks the decode desynchronised and advances minimally.
    #[test]
    fn an_unrecognised_instruction_marks_the_decode_desynchronised() {
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

    /// An instruction extending past the buffer sets `overran` and stops the walk.
    #[test]
    fn an_instruction_extending_past_the_buffer_is_reported_not_read() {
        let bytes = stream(&[0xD000_0000]); // claims 8 bytes, only 4 present
        let decoded = decode(&bytes, &table(), &operands());
        assert!(decoded.overran);
        assert!(!decoded.is_trustworthy());
    }

    /// A buffer that is not whole dwords is reported.
    #[test]
    fn a_buffer_that_is_not_whole_dwords_is_reported() {
        let mut bytes = stream(&[0x7E00_0000]);
        bytes.push(0xAB);
        let decoded = decode(&bytes, &table(), &operands());
        assert_eq!(decoded.trailing_bytes, 1);
        assert!(!decoded.is_trustworthy());
    }

    /// Coverage is the fraction of instructions recognised.
    #[test]
    fn coverage_is_the_recognised_fraction() {
        let bytes = stream(&[0x7E00_0000, 0xFFFF_FFFF, 0x7E00_0000, 0x7E00_0000]);
        let decoded = decode(&bytes, &table(), &operands());
        assert_eq!(decoded.known(), 3);
        assert_eq!(decoded.unknown(), 1);
        assert!((decoded.coverage() - 0.75).abs() < f64::EPSILON);
    }

    /// An empty shader scores full coverage.
    #[test]
    fn an_empty_shader_scores_full_coverage_rather_than_zero() {
        let decoded = decode(&[], &table(), &operands());
        assert!((decoded.coverage() - 1.0).abs() < f64::EPSILON);
        assert!(decoded.is_trustworthy());
    }

    /// Every family in the built-in table recognises its own identifying value.
    #[test]
    fn the_builtin_table_decodes_a_generated_stream_without_desynchronising() {
        // The stream is built from the table, one instruction per family, so it tracks
        // the table's target generation instead of hard-coding words.
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
