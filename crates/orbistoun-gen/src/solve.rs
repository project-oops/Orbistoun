//! The bit arithmetic, as pure functions testable with no toolchain installed.
//!
//! A mask one bit too wide drops every family member whose opcode uses that bit, and a real
//! instruction then decodes as a different one.

use std::collections::BTreeMap;

/// The longest run of bits from 31 downward that every word agrees on.
///
/// Stops at the first disagreement rather than collecting every constant bit: a mask with
/// a hole fits the samples rather than the format.
#[must_use]
pub(crate) fn prefix_mask(words: &[u32]) -> u32 {
    let Some(&first) = words.first() else {
        return 0;
    };
    let mut differing = 0_u32;
    for &word in &words[1..] {
        differing |= word ^ first;
    }
    let mut mask = 0_u32;
    for bit in (0..32).rev() {
        if differing & (1 << bit) != 0 {
            break;
        }
        mask |= 1 << bit;
    }
    mask
}

/// The shortest prefix that tells this family apart from every other declared one.
///
/// Derived across families rather than within one: bits constant within a family look
/// like identifying bits when the samples sit in one corner of the opcode range. The answer
/// is only as good as the declared families; the fixture differential catches a format with
/// no probe file.
#[must_use]
pub(crate) fn separating_mask(mine: &[u32], others: &[Vec<u32>]) -> u32 {
    for bits in 1_u32..=32 {
        let mask = 0xFFFF_FFFF_u32 << (32 - bits);
        let mut values = mine.iter().map(|w| w & mask);
        let Some(value) = values.next() else { continue };
        if !values.all(|v| v == value) {
            continue;
        }
        if others
            .iter()
            .flat_map(|family| family.iter())
            .all(|w| w & mask != value)
        {
            return mask;
        }
    }
    0xFFFF_FFFF
}

/// Where the opcode sits: constant per mnemonic, varying between mnemonics.
///
/// `None` when the samples cannot decide (one mnemonic, or a non-contiguous field), since a
/// wrong opcode position decodes a real instruction as a different one.
#[must_use]
pub(crate) fn opcode_field(samples: &[(String, Vec<u32>)], mask: u32) -> Option<(u32, u32)> {
    let mut by_mnemonic: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for (mnemonic, words) in samples {
        let &first = words.first()?;
        by_mnemonic
            .entry(mnemonic.as_str())
            .or_default()
            .push(first);
    }
    if by_mnemonic.len() < 2 {
        return None;
    }

    // Constant within every mnemonic...
    let mut constant_within = 0xFFFF_FFFF_u32;
    for words in by_mnemonic.values() {
        let first = words[0];
        let mut differing = 0_u32;
        for &word in &words[1..] {
            differing |= word ^ first;
        }
        constant_within &= !differing;
    }

    // ...and differing between them, outside the identifying bits.
    let firsts: Vec<u32> = by_mnemonic.values().map(|w| w[0]).collect();
    let mut between = 0_u32;
    for &word in &firsts[1..] {
        between |= word ^ firsts[0];
    }
    let candidate = constant_within & between & !mask;
    if candidate == 0 {
        return None;
    }

    // The span between the lowest and highest differing bit, not the set: opcodes 0x01,
    // 0x02, 0x03 and 0x41 differ only in bits 0, 1 and 6. The bits inside are constant
    // across every operand variation and below the separating prefix; an operand bit no
    // probe varied could still hide there, so the sweep runs before this is trusted.
    let shift = candidate.trailing_zeros();
    let width = (32 - candidate.leading_zeros()) - shift;
    Some((shift, width))
}

/// How many words to sweep per family.
///
/// Capped, because the sweep only needs to reach the top of the opcode range, not
/// enumerate it.
pub(crate) const SWEEP_LIMIT: u64 = 8192;

/// The candidate words to ask the disassembler about, sweeping below `mask`.
///
/// Swept from bit zero, not from the believed opcode start, so a wrong start (from probes
/// that happened not to differ in a low bit) can be corrected. Strided rather than the
/// first N values, so it reaches the top of a wide field.
#[must_use]
pub(crate) fn sweep_candidates(base: &[u32], mask: u32) -> Vec<Vec<u32>> {
    let Some(&first) = base.first() else {
        return Vec::new();
    };
    // `u64`, because a zero mask spans the whole 32-bit range plus one.
    let span = u64::from(!mask) + 1;
    let stride = (span / SWEEP_LIMIT).max(1);
    let mut out = Vec::new();
    let mut value = 0_u64;
    while value < span {
        let mut words = Vec::with_capacity(base.len());
        // The cast cannot lose information: `value < span <= 2^32`.
        #[allow(clippy::cast_possible_truncation)]
        words.push((first & mask) | (value as u32));
        words.extend_from_slice(&base[1..]);
        out.push(words);
        value += stride;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{opcode_field, prefix_mask, separating_mask, sweep_candidates};

    /// The prefix stops at the first disagreement, and does not resume after it.
    ///
    /// A mask with a hole would misfile instructions outside the samples.
    #[test]
    fn the_prefix_stops_at_the_first_disagreement() {
        // Bit 30 differs; bit 29 agrees again. The run must end at 31.
        let words = [0b1010 << 28, 0b1110 << 28];
        assert_eq!(prefix_mask(&words), 1 << 31);
    }

    /// One word agrees with itself entirely.
    #[test]
    fn a_single_word_agrees_with_itself() {
        assert_eq!(prefix_mask(&[0x1234_5678]), 0xFFFF_FFFF);
    }

    /// An empty sample set masks nothing rather than everything.
    ///
    /// `0xFFFFFFFF` would claim every bit identifies a family with no samples.
    #[test]
    fn no_words_mask_nothing() {
        assert_eq!(prefix_mask(&[]), 0);
    }

    /// The separating mask is the shortest prefix that separates.
    #[test]
    fn the_separating_mask_is_the_shortest_that_separates() {
        // Mine all start 0b10; the other family starts 0b11. Two bits is enough.
        let mine = vec![0b10 << 30, (0b10 << 30) | 0xFF];
        let others = vec![vec![0b11 << 30, (0b11 << 30) | 0x0F]];
        assert_eq!(separating_mask(&mine, &others), 0b11 << 30);
    }

    /// A family that cannot be separated claims every bit rather than a plausible few.
    ///
    /// A short mask would merge two formats; claiming everything makes the failure visible.
    #[test]
    fn an_inseparable_family_claims_everything() {
        let mine = vec![0x1234_5678];
        let others = vec![vec![0x1234_5678]];
        assert_eq!(separating_mask(&mine, &others), 0xFFFF_FFFF);
    }

    /// The opcode field is the span between the lowest and highest differing bit.
    ///
    /// Not the set of differing bits, which few mnemonics leave non-contiguous.
    #[test]
    fn the_opcode_field_is_a_span_not_a_bit_set() {
        // Opcodes 0x01, 0x02, 0x03, 0x41 at bit 8 differ in bits 8, 9 and 14.
        let samples: Vec<(String, Vec<u32>)> = [0x01, 0x02, 0x03, 0x41]
            .iter()
            .enumerate()
            .map(|(i, op)| (format!("op{i}"), vec![op << 8]))
            .collect();
        let (shift, width) = opcode_field(&samples, 0).expect("solvable");
        assert_eq!(shift, 8);
        assert_eq!(width, 7, "bits 8..=14 inclusive");
    }

    /// One mnemonic cannot locate an opcode, and says so rather than guessing.
    #[test]
    fn one_mnemonic_cannot_locate_an_opcode() {
        let samples = vec![
            ("only".to_owned(), vec![0x0000_0100]),
            ("only".to_owned(), vec![0x0000_0200]),
        ];
        assert_eq!(opcode_field(&samples, 0), None);
    }

    /// Bits inside the identifying mask are never offered as the opcode.
    #[test]
    fn identifying_bits_are_excluded_from_the_opcode() {
        let samples = vec![
            ("a".to_owned(), vec![0b1000 << 28]),
            ("b".to_owned(), vec![0b1100 << 28]),
        ];
        // The only differing bit is bit 30, which the mask claims.
        assert_eq!(opcode_field(&samples, 0b1111 << 28), None);
    }

    /// The sweep reaches the top of the range, not just the bottom of it.
    ///
    /// The stride reaches beyond the first `SWEEP_LIMIT` values of a wide field.
    #[test]
    fn the_sweep_reaches_the_top_of_the_range() {
        let mask = 0xFFFF_0000_u32;
        let candidates = sweep_candidates(&[0xABCD_0000], mask);
        assert!(!candidates.is_empty());
        let highest = candidates
            .iter()
            .map(|w| w[0] & !mask)
            .max()
            .expect("non-empty");
        assert!(
            highest > (!mask / 2),
            "swept only up to {highest:#x} of {:#x}",
            !mask
        );
        // The identifying bits are held still throughout.
        assert!(candidates.iter().all(|w| w[0] & mask == 0xABCD_0000));
    }

    /// Trailing words are carried through the sweep unchanged.
    ///
    /// Only the first word carries the opcode; the rest must stay as given.
    #[test]
    fn trailing_words_are_carried_unchanged() {
        let candidates = sweep_candidates(&[0x8000_0000, 0xDEAD_BEEF], 0xFF00_0000);
        assert!(candidates.iter().all(|w| w[1] == 0xDEAD_BEEF));
        assert!(candidates.iter().all(|w| w.len() == 2));
    }

    /// A fully-claimed mask sweeps exactly one candidate, not zero and not four billion.
    #[test]
    fn a_full_mask_sweeps_one_candidate() {
        assert_eq!(sweep_candidates(&[0x1234_5678], 0xFFFF_FFFF).len(), 1);
    }
}
