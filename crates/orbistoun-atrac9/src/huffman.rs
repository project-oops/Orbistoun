//! The Huffman codebooks scale factors and spectra are coded with.

use crate::bits::{BitReader, sign_extend};
use crate::codes::{
    SCALE_FACTORS_A_1_BITS, SCALE_FACTORS_A_1_CODES, SCALE_FACTORS_A_2_BITS,
    SCALE_FACTORS_A_2_CODES, SCALE_FACTORS_A_3_BITS, SCALE_FACTORS_A_3_CODES,
    SCALE_FACTORS_A_4_BITS, SCALE_FACTORS_A_4_CODES, SCALE_FACTORS_A_5_BITS,
    SCALE_FACTORS_A_5_CODES, SCALE_FACTORS_A_6_BITS, SCALE_FACTORS_A_6_CODES,
    SCALE_FACTORS_B_2_BITS, SCALE_FACTORS_B_2_CODES, SCALE_FACTORS_B_3_BITS,
    SCALE_FACTORS_B_3_CODES, SCALE_FACTORS_B_4_BITS, SCALE_FACTORS_B_4_CODES,
    SCALE_FACTORS_B_5_BITS, SCALE_FACTORS_B_5_CODES, SPECTRUM_A_21_BITS, SPECTRUM_A_21_CODES,
    SPECTRUM_A_22_BITS, SPECTRUM_A_22_CODES, SPECTRUM_A_23_BITS, SPECTRUM_A_23_CODES,
    SPECTRUM_A_24_BITS, SPECTRUM_A_24_CODES, SPECTRUM_A_31_BITS, SPECTRUM_A_31_CODES,
    SPECTRUM_A_32_BITS, SPECTRUM_A_32_CODES, SPECTRUM_A_33_BITS, SPECTRUM_A_33_CODES,
    SPECTRUM_A_34_BITS, SPECTRUM_A_34_CODES, SPECTRUM_A_41_BITS, SPECTRUM_A_41_CODES,
    SPECTRUM_A_42_BITS, SPECTRUM_A_42_CODES, SPECTRUM_A_43_BITS, SPECTRUM_A_43_CODES,
    SPECTRUM_A_44_BITS, SPECTRUM_A_44_CODES, SPECTRUM_A_51_BITS, SPECTRUM_A_51_CODES,
    SPECTRUM_A_52_BITS, SPECTRUM_A_52_CODES, SPECTRUM_A_53_BITS, SPECTRUM_A_53_CODES,
    SPECTRUM_A_54_BITS, SPECTRUM_A_54_CODES, SPECTRUM_A_61_BITS, SPECTRUM_A_61_CODES,
    SPECTRUM_A_62_BITS, SPECTRUM_A_62_CODES, SPECTRUM_A_63_BITS, SPECTRUM_A_63_CODES,
    SPECTRUM_A_64_BITS, SPECTRUM_A_64_CODES, SPECTRUM_A_71_BITS, SPECTRUM_A_71_CODES,
    SPECTRUM_A_72_BITS, SPECTRUM_A_72_CODES, SPECTRUM_A_73_BITS, SPECTRUM_A_73_CODES,
    SPECTRUM_A_74_BITS, SPECTRUM_A_74_CODES, SPECTRUM_B_22_BITS, SPECTRUM_B_22_CODES,
    SPECTRUM_B_23_BITS, SPECTRUM_B_23_CODES, SPECTRUM_B_24_BITS, SPECTRUM_B_24_CODES,
    SPECTRUM_B_32_BITS, SPECTRUM_B_32_CODES, SPECTRUM_B_33_BITS, SPECTRUM_B_33_CODES,
    SPECTRUM_B_34_BITS, SPECTRUM_B_34_CODES, SPECTRUM_B_42_BITS, SPECTRUM_B_42_CODES,
    SPECTRUM_B_43_BITS, SPECTRUM_B_43_CODES, SPECTRUM_B_44_BITS, SPECTRUM_B_44_CODES,
    SPECTRUM_B_52_BITS, SPECTRUM_B_52_CODES, SPECTRUM_B_53_BITS, SPECTRUM_B_53_CODES,
    SPECTRUM_B_54_BITS, SPECTRUM_B_54_CODES, SPECTRUM_B_62_BITS, SPECTRUM_B_62_CODES,
    SPECTRUM_B_63_BITS, SPECTRUM_B_63_CODES, SPECTRUM_B_64_BITS, SPECTRUM_B_64_CODES,
    SPECTRUM_B_72_BITS, SPECTRUM_B_72_CODES, SPECTRUM_B_73_BITS, SPECTRUM_B_73_CODES,
    SPECTRUM_B_74_BITS, SPECTRUM_B_74_CODES,
};
use std::sync::OnceLock;

/// One codebook: each value's code, and a table from every `max_bits`-bit prefix to the value
/// whose code it starts with (LibAtrac9 `C/src/huffCodes.c` 5-51).
#[derive(Debug)]
pub(crate) struct Codebook {
    /// Each value's code length in bits; 0 for a value with no code.
    pub(crate) bits: &'static [u8],
    /// Each value's code, right-aligned; decoding needs only the prefix table, the tests'
    /// encoder needs these.
    #[cfg(test)]
    pub(crate) codes: &'static [u16],
    /// How many coefficients one value packs.
    pub(crate) value_count: usize,
    /// `log2(value_count)`.
    pub(crate) value_count_power: u32,
    /// Each packed coefficient's width in bits.
    pub(crate) value_bits: u32,
    /// The value range, a power of two; a scale-factor delta wraps within it.
    pub(crate) value_max: i32,
    /// The longest code's length.
    pub(crate) max_bits: u32,
    /// The value each `max_bits`-bit prefix begins.
    lookup: Vec<u8>,
}

impl Codebook {
    /// A codebook over `bits` and `codes`, with its prefix table built.
    fn new(
        bits: &'static [u8],
        codes: &'static [u16],
        [
            value_count,
            value_count_power,
            value_bits,
            value_max,
            max_bits,
        ]: [u32; 5],
    ) -> Self {
        let mut lookup = vec![0_u8; 1 << max_bits];
        for (value, (&length, &code)) in bits.iter().zip(codes).enumerate() {
            if length == 0 {
                continue;
            }
            let unused = max_bits - u32::from(length);
            let start = usize::from(code) << unused;
            lookup[start..start + (1 << unused)].fill(value as u8);
        }
        Self {
            bits,
            #[cfg(test)]
            codes,
            value_count: value_count as usize,
            value_count_power,
            value_bits,
            value_max: value_max as i32,
            max_bits,
            lookup,
        }
    }

    /// Reads one value's code, answering the value: as coded, or sign-extended from
    /// `value_bits` when `signed`.
    pub(crate) fn read(&self, reader: &mut BitReader<'_>, signed: bool) -> i32 {
        let value = self.lookup[reader.peek(self.max_bits) as usize];
        reader.skip(usize::from(self.bits[usize::from(value)]));
        if signed {
            sign_extend(i32::from(value), self.value_bits)
        } else {
            i32::from(value)
        }
    }

    /// Unpacks `values`, each holding `value_count` signed coefficients least significant
    /// first, into `out`.
    pub(crate) fn unpack(&self, values: &[i32], out: &mut [i32]) {
        let mask = (1 << self.value_bits) - 1;
        let mut at = 0;
        for &packed in values {
            let mut value = packed;
            for _ in 0..self.value_count {
                out[at] = sign_extend(value & mask, self.value_bits);
                value >>= self.value_bits;
                at += 1;
            }
        }
    }
}

/// Every codebook the format uses.
#[derive(Debug)]
pub(crate) struct Codebooks {
    /// Scale-factor deltas, by width in bits (1 to 6).
    pub(crate) scale_unsigned: [Option<Codebook>; 7],
    /// Scale-factor distances from a baseline, by width in bits (2 to 5).
    pub(crate) scale_signed: [Option<Codebook>; 6],
    /// Spectra, by codebook set (0 or 1), precision (2 to 7) and the unit's codebook index.
    pub(crate) spectrum: [[[Option<Codebook>; 4]; 8]; 2],
}

/// The codebooks, built on first use.
pub(crate) fn codebooks() -> &'static Codebooks {
    static BOOKS: OnceLock<Codebooks> = OnceLock::new();
    BOOKS.get_or_init(build)
}

/// Builds every codebook with the parameters LibAtrac9 gives each (`C/src/huffCodes.c`
/// 1324-1424): values per group, its log, value width, value range and longest code.
#[allow(
    clippy::too_many_lines,
    reason = "one table, laid out as the reference's so each row can be checked against it"
)]
fn build() -> Codebooks {
    let book = |bits, codes, params| Some(Codebook::new(bits, codes, params));
    let scale_unsigned = [
        None,
        book(
            &SCALE_FACTORS_A_1_BITS,
            &SCALE_FACTORS_A_1_CODES,
            [1, 0, 1, 2, 1],
        ),
        book(
            &SCALE_FACTORS_A_2_BITS,
            &SCALE_FACTORS_A_2_CODES,
            [1, 0, 2, 4, 3],
        ),
        book(
            &SCALE_FACTORS_A_3_BITS,
            &SCALE_FACTORS_A_3_CODES,
            [1, 0, 3, 8, 6],
        ),
        book(
            &SCALE_FACTORS_A_4_BITS,
            &SCALE_FACTORS_A_4_CODES,
            [1, 0, 4, 16, 8],
        ),
        book(
            &SCALE_FACTORS_A_5_BITS,
            &SCALE_FACTORS_A_5_CODES,
            [1, 0, 5, 32, 8],
        ),
        book(
            &SCALE_FACTORS_A_6_BITS,
            &SCALE_FACTORS_A_6_CODES,
            [1, 0, 6, 64, 8],
        ),
    ];
    let scale_signed = [
        None,
        None,
        book(
            &SCALE_FACTORS_B_2_BITS,
            &SCALE_FACTORS_B_2_CODES,
            [1, 0, 2, 4, 2],
        ),
        book(
            &SCALE_FACTORS_B_3_BITS,
            &SCALE_FACTORS_B_3_CODES,
            [1, 0, 3, 8, 6],
        ),
        book(
            &SCALE_FACTORS_B_4_BITS,
            &SCALE_FACTORS_B_4_CODES,
            [1, 0, 4, 16, 8],
        ),
        book(
            &SCALE_FACTORS_B_5_BITS,
            &SCALE_FACTORS_B_5_CODES,
            [1, 0, 5, 32, 8],
        ),
    ];
    let set_a = [
        [None, None, None, None],
        [None, None, None, None],
        [
            book(&SPECTRUM_A_21_BITS, &SPECTRUM_A_21_CODES, [2, 1, 2, 4, 3]),
            book(&SPECTRUM_A_22_BITS, &SPECTRUM_A_22_CODES, [4, 2, 2, 4, 8]),
            book(&SPECTRUM_A_23_BITS, &SPECTRUM_A_23_CODES, [4, 2, 2, 4, 9]),
            book(&SPECTRUM_A_24_BITS, &SPECTRUM_A_24_CODES, [4, 2, 2, 4, 10]),
        ],
        [
            book(&SPECTRUM_A_31_BITS, &SPECTRUM_A_31_CODES, [2, 1, 3, 8, 7]),
            book(&SPECTRUM_A_32_BITS, &SPECTRUM_A_32_CODES, [2, 1, 3, 8, 7]),
            book(&SPECTRUM_A_33_BITS, &SPECTRUM_A_33_CODES, [2, 1, 3, 8, 8]),
            book(&SPECTRUM_A_34_BITS, &SPECTRUM_A_34_CODES, [2, 1, 3, 8, 10]),
        ],
        [
            book(&SPECTRUM_A_41_BITS, &SPECTRUM_A_41_CODES, [2, 1, 4, 16, 9]),
            book(&SPECTRUM_A_42_BITS, &SPECTRUM_A_42_CODES, [2, 1, 4, 16, 10]),
            book(&SPECTRUM_A_43_BITS, &SPECTRUM_A_43_CODES, [2, 1, 4, 16, 10]),
            book(&SPECTRUM_A_44_BITS, &SPECTRUM_A_44_CODES, [2, 1, 4, 16, 10]),
        ],
        [
            book(&SPECTRUM_A_51_BITS, &SPECTRUM_A_51_CODES, [1, 0, 5, 32, 6]),
            book(&SPECTRUM_A_52_BITS, &SPECTRUM_A_52_CODES, [1, 0, 5, 32, 6]),
            book(&SPECTRUM_A_53_BITS, &SPECTRUM_A_53_CODES, [1, 0, 5, 32, 7]),
            book(&SPECTRUM_A_54_BITS, &SPECTRUM_A_54_CODES, [1, 0, 5, 32, 8]),
        ],
        [
            book(&SPECTRUM_A_61_BITS, &SPECTRUM_A_61_CODES, [1, 0, 6, 64, 7]),
            book(&SPECTRUM_A_62_BITS, &SPECTRUM_A_62_CODES, [1, 0, 6, 64, 7]),
            book(&SPECTRUM_A_63_BITS, &SPECTRUM_A_63_CODES, [1, 0, 6, 64, 8]),
            book(&SPECTRUM_A_64_BITS, &SPECTRUM_A_64_CODES, [1, 0, 6, 64, 9]),
        ],
        [
            book(&SPECTRUM_A_71_BITS, &SPECTRUM_A_71_CODES, [1, 0, 7, 128, 8]),
            book(&SPECTRUM_A_72_BITS, &SPECTRUM_A_72_CODES, [1, 0, 7, 128, 8]),
            book(&SPECTRUM_A_73_BITS, &SPECTRUM_A_73_CODES, [1, 0, 7, 128, 9]),
            book(
                &SPECTRUM_A_74_BITS,
                &SPECTRUM_A_74_CODES,
                [1, 0, 7, 128, 10],
            ),
        ],
    ];
    let set_b = [
        [None, None, None, None],
        [None, None, None, None],
        [
            None,
            book(&SPECTRUM_B_22_BITS, &SPECTRUM_B_22_CODES, [4, 2, 2, 4, 10]),
            book(&SPECTRUM_B_23_BITS, &SPECTRUM_B_23_CODES, [4, 2, 2, 4, 10]),
            book(&SPECTRUM_B_24_BITS, &SPECTRUM_B_24_CODES, [4, 2, 2, 4, 10]),
        ],
        [
            None,
            book(&SPECTRUM_B_32_BITS, &SPECTRUM_B_32_CODES, [2, 1, 3, 8, 9]),
            book(&SPECTRUM_B_33_BITS, &SPECTRUM_B_33_CODES, [2, 1, 3, 8, 10]),
            book(&SPECTRUM_B_34_BITS, &SPECTRUM_B_34_CODES, [2, 1, 3, 8, 10]),
        ],
        [
            None,
            book(&SPECTRUM_B_42_BITS, &SPECTRUM_B_42_CODES, [2, 1, 4, 16, 10]),
            book(&SPECTRUM_B_43_BITS, &SPECTRUM_B_43_CODES, [2, 1, 4, 16, 10]),
            book(&SPECTRUM_B_44_BITS, &SPECTRUM_B_44_CODES, [2, 1, 4, 16, 10]),
        ],
        [
            None,
            book(&SPECTRUM_B_52_BITS, &SPECTRUM_B_52_CODES, [1, 0, 5, 32, 7]),
            book(&SPECTRUM_B_53_BITS, &SPECTRUM_B_53_CODES, [1, 0, 5, 32, 8]),
            book(&SPECTRUM_B_54_BITS, &SPECTRUM_B_54_CODES, [1, 0, 5, 32, 9]),
        ],
        [
            None,
            book(&SPECTRUM_B_62_BITS, &SPECTRUM_B_62_CODES, [1, 0, 6, 64, 8]),
            book(&SPECTRUM_B_63_BITS, &SPECTRUM_B_63_CODES, [1, 0, 6, 64, 9]),
            book(&SPECTRUM_B_64_BITS, &SPECTRUM_B_64_CODES, [1, 0, 6, 64, 10]),
        ],
        [
            None,
            book(&SPECTRUM_B_72_BITS, &SPECTRUM_B_72_CODES, [1, 0, 7, 128, 9]),
            book(
                &SPECTRUM_B_73_BITS,
                &SPECTRUM_B_73_CODES,
                [1, 0, 7, 128, 10],
            ),
            book(
                &SPECTRUM_B_74_BITS,
                &SPECTRUM_B_74_CODES,
                [1, 0, 7, 128, 10],
            ),
        ],
    ];
    Codebooks {
        scale_unsigned,
        scale_signed,
        spectrum: [set_a, set_b],
    }
}

#[cfg(test)]
mod tests {
    use super::codebooks;
    use crate::bits::BitReader;

    /// Every codebook is a prefix code within its longest length: no two values' codes overlap
    /// in the prefix table, and every value with a code reads back as itself.
    #[test]
    fn every_code_reads_back_as_its_value() {
        let books = codebooks();
        let all = books
            .scale_unsigned
            .iter()
            .chain(&books.scale_signed)
            .chain(books.spectrum.iter().flatten().flatten())
            .flatten();
        let mut count = 0;
        for book in all {
            count += 1;
            let mut claimed = vec![false; 1 << book.max_bits];
            for (value, (&length, &code)) in book.bits.iter().zip(book.codes).enumerate() {
                if length == 0 {
                    continue;
                }
                let unused = book.max_bits - u32::from(length);
                let start = usize::from(code) << unused;
                for slot in &mut claimed[start..start + (1 << unused)] {
                    assert!(!*slot, "codes overlap in a codebook");
                    *slot = true;
                }
                let bytes = (u32::from(code) << (32 - u32::from(length))).to_be_bytes();
                let mut reader = BitReader::new(&bytes);
                assert_eq!(book.read(&mut reader, false), value as i32);
                assert_eq!(reader.position(), usize::from(length));
            }
        }
        assert_eq!(count, 6 + 4 + 24 + 18, "every codebook the reference lists");
    }
}
