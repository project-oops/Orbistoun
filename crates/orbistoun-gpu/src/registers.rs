//! Register writes, and the shader addresses hiding among them.
//!
//! # The link between the two halves of the GPU work
//!
//! A submission does not contain shaders. It contains *addresses* of shaders, written
//! into hardware registers by ordinary register-write packets. Extracting those
//! addresses is what lets a captured command stream feed the shader corpus, and
//! without it the packet walker and the shader decoder are two tools that never meet.
//!
//! # Mechanism and interpretation are separated on purpose
//!
//! Pulling register writes out of a packet stream is **structural**: a type-0 packet
//! writes consecutive registers from a base in its header, and a `SET_*_REG` packet
//! writes consecutive registers from an index in its first body word. That is correct
//! regardless of what any particular register means.
//!
//! Deciding that register `0x2C0C` holds the low half of a fragment shader's address
//! is a **hypothesis**, and unlike the shader encoding table there is no reference
//! implementation to check it against cheaply. So it lives in
//! `data/packets.toml`, it is correctable without a rebuild, and what comes out is
//! reported as a *candidate* address rather than as a fact.
//!
//! Getting that separation right matters more than getting the hypothesis right: the
//! mechanism will still be correct when the table is fixed.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::packet::{PacketKind, PacketWalk};

/// Why a packet vocabulary could not be loaded.
///
/// Its own type rather than the backend's: a malformed data file and a backend that
/// cannot render something are unrelated failures, and sharing an error between them
/// would make a caller handle one while thinking about the other.
#[derive(Debug, thiserror::Error)]
pub enum VocabularyError {
    /// The file could not be parsed.
    #[error("packet vocabulary: {0}")]
    Malformed(String),
    /// An entry is individually well-formed and means nothing.
    #[error("packet vocabulary: {0}")]
    Invalid(String),
}

/// One register write observed in a submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegisterWrite {
    /// Byte offset of the packet that wrote it, so a finding can be traced back.
    pub packet_offset: u32,
    /// Register index.
    pub register: u32,
    /// Value written.
    pub value: u32,
}

/// A shader address recovered from a pair of register writes.
///
/// Called a candidate rather than an address because the register mapping it rests on
/// is unverified. Nothing follows one of these blindly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderCandidate {
    /// Pipeline stage the mapping attributes it to. Reported, never dispatched on.
    pub stage: String,
    /// The reassembled address.
    pub address: u64,
    /// Where the low half was written.
    pub packet_offset: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct OpcodeName {
    value: u8,
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RegisterWriteKind {
    opcode: u8,
    #[expect(dead_code, reason = "kept so the table reads against the reference")]
    name: String,
    base: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct ShaderAddressRegister {
    register: u32,
    stage: String,
    half: String,
}

#[derive(Debug, Deserialize, Default)]
struct VocabularyFile {
    #[serde(default)]
    opcode: Vec<OpcodeName>,
    #[serde(default)]
    register_write: Vec<RegisterWriteKind>,
    #[serde(default)]
    shader_address: Vec<ShaderAddressRegister>,
}

/// Names and mappings for the command stream.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    opcodes: BTreeMap<u8, String>,
    register_bases: BTreeMap<u8, u32>,
    /// register -> (stage, is_high_half)
    shader_registers: BTreeMap<u32, (String, bool)>,
}

impl Vocabulary {
    /// Parses a vocabulary from TOML.
    pub fn load(toml_text: &str) -> Result<Self, VocabularyError> {
        let file: VocabularyFile =
            toml::from_str(toml_text).map_err(|e| VocabularyError::Malformed(e.to_string()))?;

        let mut shader_registers = BTreeMap::new();
        for entry in file.shader_address {
            // Anything that is not the low half is treated as the high half, and an
            // unrecognised value would silently become one. Refusing is better: a
            // typo here produces addresses with their halves swapped, which look
            // plausible and are wrong by four billion.
            let high = match entry.half.as_str() {
                "low" => false,
                "high" => true,
                other => {
                    return Err(VocabularyError::Invalid(format!(
                        "shader address register {:#x} has half {other:?}, expected low or high",
                        entry.register
                    )));
                }
            };
            shader_registers.insert(entry.register, (entry.stage, high));
        }

        Ok(Self {
            opcodes: file.opcode.into_iter().map(|o| (o.value, o.name)).collect(),
            register_bases: file
                .register_write
                .into_iter()
                .map(|r| (r.opcode, r.base))
                .collect(),
            shader_registers,
        })
    }

    /// The built-in vocabulary.
    pub fn builtin() -> Result<Self, VocabularyError> {
        Self::load(include_str!("../data/packets.toml"))
    }

    /// A readable name for a type-3 opcode, or `None` if the table has no entry.
    ///
    /// `None` rather than a placeholder string: a report should be able to show the
    /// raw value for an opcode nobody has named yet, and an invented name would hide
    /// that the vocabulary has a gap.
    pub fn opcode_name(&self, opcode: u8) -> Option<&str> {
        self.opcodes.get(&opcode).map(String::as_str)
    }

    /// Whether this opcode writes registers, and from what base.
    pub fn register_base(&self, opcode: u8) -> Option<u32> {
        self.register_bases.get(&opcode).copied()
    }

    /// How many shader address registers are mapped.
    pub fn shader_register_count(&self) -> usize {
        self.shader_registers.len()
    }

    /// Every shader address register, as `register -> (stage, is high half)`.
    ///
    /// Exposed so a consumer can build a submission that names one, and so a report can
    /// show what the vocabulary claims to know. Read-only: this is a transcription and
    /// the least certain thing in the file, so it is worth being inspectable.
    pub fn shader_registers(&self) -> impl Iterator<Item = (&u32, &(String, bool))> {
        self.shader_registers.iter()
    }

    /// The register-writing opcode that reaches a given register, and its base.
    ///
    /// Several opcodes write registers, each to a different class with its own base, and
    /// which one reaches a register is decided by the register - not by which opcode
    /// happens to come first. Asking for "an opcode that writes registers" and using it
    /// for any register underflows the offset for every register below its base, which
    /// is a subtraction that happens to be checked here and would be a silently wrong
    /// packet anywhere else.
    ///
    /// The closest base at or below the register, so a register in two classes' ranges
    /// resolves to the nearer one.
    pub fn opcode_for_register(&self, register: u32) -> Option<(u8, u32)> {
        self.register_bases
            .iter()
            .filter(|(_, base)| **base <= register)
            .max_by_key(|(_, base)| **base)
            .map(|(opcode, base)| (*opcode, *base))
    }
}

/// Pulls every register write out of a walked submission.
///
/// `body` is the full submitted buffer, needed because a packet's values live after
/// its header and [`PacketWalk`] records positions rather than copying them.
pub fn register_writes(
    walk: &PacketWalk,
    body: &[u8],
    vocabulary: &Vocabulary,
) -> Vec<RegisterWrite> {
    let mut writes = Vec::new();

    for packet in &walk.packets {
        let start = packet.body_offset() as usize;
        let length = packet.body_length() as usize;
        let Some(words) = read_words(body, start, length) else {
            continue;
        };

        match packet.kind {
            PacketKind::RegisterWrite { base_register } => {
                // The header names the first register; the body is the values.
                for (index, value) in words.iter().enumerate() {
                    writes.push(RegisterWrite {
                        packet_offset: packet.offset,
                        register: u32::from(base_register) + u32::try_from(index).unwrap_or(0),
                        value: *value,
                    });
                }
            }
            PacketKind::Command { opcode } => {
                let Some(base) = vocabulary.register_base(opcode) else {
                    continue;
                };
                // Body word zero is the index; the rest are values.
                let Some((offset, values)) = words.split_first() else {
                    continue;
                };
                for (index, value) in values.iter().enumerate() {
                    writes.push(RegisterWrite {
                        packet_offset: packet.offset,
                        register: base
                            .wrapping_add(*offset)
                            .wrapping_add(u32::try_from(index).unwrap_or(0)),
                        value: *value,
                    });
                }
            }
            PacketKind::Filler | PacketKind::Reserved => {}
        }
    }

    writes
}

/// Reassembles shader addresses from register writes.
///
/// An address needs both halves. A stage with only one half seen is **skipped rather
/// than half-formed**: an address missing its high word points into the bottom four
/// gigabytes and would look like an ordinary low address rather than like a mistake.
///
/// Later writes win, because a submission legitimately rebinds a stage several times
/// and the last one before a draw is the one that mattered.
pub fn shader_candidates(
    writes: &[RegisterWrite],
    vocabulary: &Vocabulary,
) -> Vec<ShaderCandidate> {
    // stage -> (low, high, offset of the low write)
    let mut halves: BTreeMap<&str, (Option<u32>, Option<u32>, u32)> = BTreeMap::new();

    for write in writes {
        let Some((stage, is_high)) = vocabulary.shader_registers.get(&write.register) else {
            continue;
        };
        let entry = halves.entry(stage.as_str()).or_insert((None, None, 0));
        if *is_high {
            entry.1 = Some(write.value);
        } else {
            entry.0 = Some(write.value);
            entry.2 = write.packet_offset;
        }
    }

    halves
        .into_iter()
        .filter_map(|(stage, (low, high, offset))| {
            let (low, high) = (low?, high?);
            Some(ShaderCandidate {
                stage: stage.to_owned(),
                address: (u64::from(high) << 32) | u64::from(low),
                packet_offset: offset,
            })
        })
        .collect()
}

/// Reads `length` bytes at `start` as little-endian words.
fn read_words(body: &[u8], start: usize, length: usize) -> Option<Vec<u32>> {
    let end = start.checked_add(length)?;
    let slice = body.get(start..end)?;
    Some(
        slice
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

#[cfg(test)]
mod tests {

    #[test]
    fn every_shader_address_is_a_consecutive_low_high_pair() {
        // D091 calls the shader-address register map "a hypothesis with no oracle", and
        // for the register *numbers* that is still true - they are transcribed, and the
        // instruction-set reference names the registers without giving their offsets.
        //
        // It does say something checkable about their *shape*, though: a shader's start
        // address comes from `SPI_SHADER_PGM_LO/HI`, per stage, as a pair. So each stage
        // must have exactly one of each half and the high must sit immediately above the
        // low. A transposed digit or a dropped half breaks that, and those are the
        // transcription mistakes this table is most exposed to.
        //
        // It does not make the numbers right. It makes one class of being wrong loud.
        let table = Vocabulary::builtin().expect("packets");

        let mut by_stage: std::collections::BTreeMap<&str, (Option<u32>, Option<u32>)> =
            std::collections::BTreeMap::new();
        for (register, (stage, is_high)) in &table.shader_registers {
            let slot = by_stage.entry(stage.as_str()).or_default();
            if *is_high {
                slot.1 = Some(*register);
            } else {
                slot.0 = Some(*register);
            }
        }

        assert!(
            !by_stage.is_empty(),
            "the table names no shader addresses at all"
        );
        for (stage, (low, high)) in by_stage {
            let low = low.unwrap_or_else(|| panic!("{stage} has a high half and no low"));
            let high = high.unwrap_or_else(|| panic!("{stage} has a low half and no high"));
            assert_eq!(
                high,
                low + 1,
                "{stage}: the reference forms an address from a consecutive LO/HI pair, so                  {high:#x} should be one above {low:#x}"
            );
        }
    }
    use super::{Vocabulary, register_writes, shader_candidates};
    use crate::packet::walk;

    fn stream(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    fn command(opcode: u8, body_dwords: u32) -> u32 {
        (3 << 30) | ((body_dwords - 1) << 16) | (u32::from(opcode) << 8)
    }

    fn register_packet(base: u16, body_dwords: u32) -> u32 {
        ((body_dwords - 1) << 16) | u32::from(base)
    }

    fn vocabulary() -> Vocabulary {
        Vocabulary::load(
            r#"
            [[opcode]]
            value = 0x76
            name = "SET_SH_REG"

            [[register_write]]
            opcode = 0x76
            name = "SET_SH_REG"
            base = 0x2C00

            [[shader_address]]
            register = 0x2C0C
            stage = "fragment"
            half = "low"

            [[shader_address]]
            register = 0x2C0D
            stage = "fragment"
            half = "high"
            "#,
        )
        .expect("vocabulary")
    }

    #[test]
    fn a_type_zero_packet_writes_consecutive_registers_from_its_header() {
        let bytes = stream(&[register_packet(0x30, 3), 0xAAAA, 0xBBBB, 0xCCCC]);
        let writes = register_writes(&walk(&bytes), &bytes, &vocabulary());
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[0].register, 0x30);
        assert_eq!(writes[1].register, 0x31);
        assert_eq!(writes[2].value, 0xCCCC);
    }

    #[test]
    fn a_set_register_packet_takes_its_index_from_the_first_body_word() {
        // The index is data, not header. Reading it as a value instead would write the
        // register number into a register and shift everything by one.
        let bytes = stream(&[command(0x76, 3), 0x0C, 0x1111, 0x2222]);
        let writes = register_writes(&walk(&bytes), &bytes, &vocabulary());
        assert_eq!(writes.len(), 2, "the index word is not a value");
        assert_eq!(writes[0].register, 0x2C0C, "base plus index");
        assert_eq!(writes[0].value, 0x1111);
        assert_eq!(writes[1].register, 0x2C0D);
    }

    #[test]
    fn a_packet_the_vocabulary_does_not_know_writes_nothing() {
        // Guessing a base for an unknown opcode would attribute its body to registers
        // that were never written, which is worse than recording nothing.
        let bytes = stream(&[command(0x2D, 2), 0x00, 0x1111]);
        let writes = register_writes(&walk(&bytes), &bytes, &vocabulary());
        assert!(writes.is_empty());
    }

    #[test]
    fn both_halves_reassemble_into_one_address() {
        let bytes = stream(&[command(0x76, 3), 0x0C, 0x8000_0000, 0x0000_00FF]);
        let walked = walk(&bytes);
        let writes = register_writes(&walked, &bytes, &vocabulary());
        let candidates = shader_candidates(&writes, &vocabulary());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].stage, "fragment");
        assert_eq!(candidates[0].address, 0x0000_00FF_8000_0000);
    }

    #[test]
    fn a_lone_half_produces_no_candidate() {
        // An address missing its high word points into the bottom four gigabytes and
        // reads as an ordinary low address rather than as a mistake. Skipping it keeps
        // a truncated stream from producing a plausible wrong answer.
        let bytes = stream(&[command(0x76, 2), 0x0C, 0x8000_0000]);
        let walked = walk(&bytes);
        let writes = register_writes(&walked, &bytes, &vocabulary());
        assert!(shader_candidates(&writes, &vocabulary()).is_empty());
    }

    #[test]
    fn a_later_bind_replaces_an_earlier_one() {
        // A submission rebinds a stage several times; the last write before a draw is
        // the one that mattered.
        let bytes = stream(&[
            command(0x76, 3),
            0x0C,
            0x1111_1111,
            0x0000_0001,
            command(0x76, 3),
            0x0C,
            0x2222_2222,
            0x0000_0002,
        ]);
        let walked = walk(&bytes);
        let writes = register_writes(&walked, &bytes, &vocabulary());
        let candidates = shader_candidates(&writes, &vocabulary());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].address, 0x0000_0002_2222_2222);
    }

    #[test]
    fn a_half_that_is_neither_low_nor_high_is_refused() {
        // Defaulting an unrecognised value to "high" would swap the halves of every
        // address it touched - plausible-looking values, wrong by four billion.
        let result = Vocabulary::load(
            r#"
            [[shader_address]]
            register = 0x2C0C
            stage = "fragment"
            half = "middle"
            "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn the_builtin_vocabulary_loads_and_names_things() {
        let vocabulary = Vocabulary::builtin().expect("builtin");
        assert_eq!(vocabulary.opcode_name(0x2D), Some("DRAW_INDEX_AUTO"));
        assert!(vocabulary.register_base(0x76).is_some());
        assert!(vocabulary.shader_register_count() >= 2);
        // An unnamed opcode reports as unnamed rather than inventing a label, so a gap
        // in the vocabulary stays visible in a report.
        assert_eq!(vocabulary.opcode_name(0xFE), None);
    }
}
