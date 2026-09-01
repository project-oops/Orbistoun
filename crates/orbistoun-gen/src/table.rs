//! Reading the encoding table the decoder uses, and classifying an instruction with it.
//!
//! **Read rather than duplicated.** The family and opcode of an instruction are decided by
//! exactly one set of rules, and a second copy of them here would drift from the decoder
//! silently.
//!
//! What *is* duplicated is [`classify`], which implements the same rule the decoder
//! implements in Rust - and the duplication is worth naming rather than hiding. The two
//! agreeing is what lets a generated table be checked against the code that reads it. The
//! two *drifting* would be hard to see, because each stays self-consistent: reading only
//! the contiguous part of a split opcode here while the decoder reads both halves would
//! classify a half-precision variant as its counterpart, and emit a second name for an
//! opcode that already has one. The name table refuses a duplicate, so that particular
//! drift surfaces loudly. Not every drift would.

use std::path::Path;

use anyhow::{Context, Result};

/// Where an opcode's bits sit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Field {
    /// Bit position the field starts at.
    pub(crate) shift: u32,
    /// How many bits it occupies.
    pub(crate) width: u32,
    /// Which word of the instruction it lives in. Zero for the first.
    pub(crate) word: usize,
}

/// One encoding family, as the decoder's table declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Encoding {
    /// Family name - `VOP3`, `SOPK`.
    pub(crate) name: String,
    /// Bits that identify the family.
    pub(crate) mask: u32,
    /// What those bits hold.
    pub(crate) value: u32,
    /// The opcode's contiguous part, always in the first word.
    pub(crate) opcode: Field,
    /// A second, higher part of the opcode, when it is split across words.
    pub(crate) extension: Option<Field>,
}

/// Reads the committed encoding table.
pub(crate) fn load(path: &Path) -> Result<Vec<Encoding>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the encoding table at {}", path.display()))?;
    parse(&text)
}

/// Parses an encoding table.
pub(crate) fn parse(text: &str) -> Result<Vec<Encoding>> {
    let document: toml::Value = text.parse().context("parsing the encoding table")?;
    let rows = document
        .get("encoding")
        .and_then(toml::Value::as_array)
        .context("the encoding table has no [[encoding]] rows")?;

    let mut out = Vec::new();
    for row in rows {
        // A row missing any of these is skipped rather than fatal, which is what the
        // generator this replaced did: the table carries rows that describe a boundary and
        // nothing else, and demanding a full opcode from all of them would reject the file.
        let (Some(name), Some(mask), Some(value), Some(opcode)) = (
            row.get("name").and_then(toml::Value::as_str),
            row.get("mask").and_then(hex_u32),
            row.get("value").and_then(hex_u32),
            row.get("opcode").and_then(|f| field(f, 0)),
        ) else {
            continue;
        };
        out.push(Encoding {
            name: name.to_owned(),
            mask,
            value,
            opcode,
            extension: row.get("opcode_extension").and_then(|f| field(f, 0)),
        });
    }
    anyhow::ensure!(!out.is_empty(), "no usable rows in the encoding table");

    // Most specific first, exactly as the loader orders it. Two families can share a
    // prefix, and the narrower mask must not claim an instruction the wider one identifies.
    out.sort_by_key(|e| std::cmp::Reverse(e.mask.count_ones()));
    Ok(out)
}

/// `"0xFC000000"` as a number. Quoted in the table, because TOML has no hex integer.
fn hex_u32(value: &toml::Value) -> Option<u32> {
    let text = value.as_str()?;
    u32::from_str_radix(text.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
}

/// `{ shift = 16, width = 3, word = 1 }`.
fn field(value: &toml::Value, default_word: usize) -> Option<Field> {
    let shift = u32::try_from(value.get("shift")?.as_integer()?).ok()?;
    let width = u32::try_from(value.get("width")?.as_integer()?).ok()?;
    let word = value
        .get("word")
        .and_then(toml::Value::as_integer)
        .and_then(|w| usize::try_from(w).ok())
        .unwrap_or(default_word);
    Some(Field { shift, width, word })
}

/// The family and opcode of an instruction, from all of its words.
///
/// **Takes the whole instruction rather than its first word**, because an opcode is not
/// always kept in one piece: the typed-buffer family puts three bits at 18:16 of the first
/// word and a fourth at bit 53, which is bit 21 of the second.
#[must_use]
pub(crate) fn classify(words: &[u32], encodings: &[Encoding]) -> Option<(String, u32)> {
    let word = words.first().copied().unwrap_or(0);
    for encoding in encodings {
        if word & encoding.mask != encoding.value {
            continue;
        }
        let mask = if encoding.opcode.width < 32 {
            (1_u32 << encoding.opcode.width) - 1
        } else {
            0xFFFF_FFFF
        };
        let mut opcode = (word >> encoding.opcode.shift) & mask;
        if let Some(extension) = encoding.extension
            && let Some(&high_word) = words.get(extension.word)
        {
            let high = (high_word >> extension.shift) & ((1_u32 << extension.width) - 1);
            opcode |= high << encoding.opcode.width;
        }
        return Some((encoding.name.clone(), opcode));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Field, classify, parse};

    const TABLE: &str = r#"
[[encoding]]
name = "WIDE"
mask = "0xFC000000"
value = "0xE8000000"
opcode = { shift = 16, width = 3 }
opcode_extension = { shift = 21, width = 1, word = 1 }

[[encoding]]
name = "NARROW"
mask = "0x80000000"
value = "0x00000000"
opcode = { shift = 8, width = 4 }

[[encoding]]
name = "BOUNDARY_ONLY"
mask = "0xFF000000"
value = "0xFF000000"
"#;

    /// A row with no opcode is skipped, not rejected.
    ///
    /// The table carries rows that describe an instruction *boundary* and nothing else.
    /// Demanding a full opcode from every row would refuse the committed file.
    #[test]
    fn a_row_describing_only_a_boundary_is_skipped() {
        let table = parse(TABLE).expect("parses");
        assert_eq!(table.len(), 2);
        assert!(table.iter().all(|e| e.name != "BOUNDARY_ONLY"));
    }

    /// The most specific mask is tried first.
    ///
    /// Two families can share a prefix. If the narrower mask were tried first it would
    /// claim instructions the wider one identifies, and every one of them would be filed
    /// under the wrong family.
    #[test]
    fn the_most_specific_mask_is_tried_first() {
        let table = parse(TABLE).expect("parses");
        assert_eq!(table[0].name, "WIDE", "6 bits before 1 bit");
    }

    /// A split opcode reads both halves.
    ///
    /// **This is the case the whole `classify` duplication exists for.** Reading only the
    /// contiguous part would give a half-precision variant the same opcode as its
    /// counterpart - the two differ in the extension bit and nothing else.
    #[test]
    fn a_split_opcode_reads_both_words() {
        let table = parse(TABLE).expect("parses");
        // First word: family bits plus opcode 0b101 at bit 16.
        let first = 0xE800_0000 | (0b101 << 16);
        let low = classify(&[first, 0x0000_0000], &table).expect("classifies");
        let high = classify(&[first, 1 << 21], &table).expect("classifies");
        assert_eq!(low, ("WIDE".to_owned(), 0b101));
        assert_eq!(
            high,
            ("WIDE".to_owned(), 0b1101),
            "the fourth bit is bit 53"
        );
        assert_ne!(low.1, high.1, "the variants must not share an opcode");
    }

    /// A missing continuation word leaves the low half alone rather than reading rubbish.
    #[test]
    fn a_missing_continuation_word_is_not_invented() {
        let table = parse(TABLE).expect("parses");
        let first = 0xE800_0000 | (0b011 << 16);
        assert_eq!(
            classify(&[first], &table),
            Some(("WIDE".to_owned(), 0b011)),
            "a truncated instruction keeps the bits it does have"
        );
    }

    /// An instruction matching nothing is unclassified, not assigned a plausible family.
    #[test]
    fn an_unmatched_instruction_is_unclassified() {
        let table = vec![super::Encoding {
            name: "ONLY".to_owned(),
            mask: 0xFF00_0000,
            value: 0xAA00_0000,
            opcode: Field {
                shift: 0,
                width: 8,
                word: 0,
            },
            extension: None,
        }];
        assert_eq!(classify(&[0x1234_5678], &table), None);
    }

    /// Hex strings are read as hex, not as decimal or as text.
    #[test]
    fn masks_are_read_as_hexadecimal() {
        let table = parse(TABLE).expect("parses");
        let wide = table.iter().find(|e| e.name == "WIDE").expect("present");
        assert_eq!(wide.mask, 0xFC00_0000);
        assert_eq!(wide.value, 0xE800_0000);
    }
}
