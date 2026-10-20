//! Reading a frame's bits, most significant first.

/// A reader over a frame's bytes. Bits past the end read as zero, and [`BitReader::overran`]
/// says whether any were read, so a truncated frame is refused rather than decoded from
/// padding.
#[derive(Debug)]
pub(crate) struct BitReader<'a> {
    /// The frame.
    data: &'a [u8],
    /// The next bit to read, counted from the first byte's most significant bit.
    position: usize,
}

impl<'a> BitReader<'a> {
    /// A reader at the start of `data`.
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    /// The next bit's position.
    pub(crate) fn position(&self) -> usize {
        self.position
    }

    /// Whether a read has gone past the end of the data.
    pub(crate) fn overran(&self) -> bool {
        self.position > self.data.len() * 8
    }

    /// The next `bits` bits, up to 32, as an unsigned value, without moving past them.
    pub(crate) fn peek(&self, bits: u32) -> u32 {
        let mut value = 0_u32;
        for i in 0..bits as usize {
            let at = self.position + i;
            let bit = self
                .data
                .get(at / 8)
                .map_or(0, |byte| (byte >> (7 - at % 8)) & 1);
            value = value << 1 | u32::from(bit);
        }
        value
    }

    /// The next `bits` bits as an unsigned value.
    pub(crate) fn read(&mut self, bits: u32) -> i32 {
        let value = self.peek(bits);
        self.skip(bits as usize);
        value as i32
    }

    /// The next `bits` bits as a two's complement value.
    pub(crate) fn read_signed(&mut self, bits: u32) -> i32 {
        sign_extend(self.read(bits), bits)
    }

    /// The next `bits` bits as an offset-binary value: unsigned, less half the range.
    pub(crate) fn read_offset_binary(&mut self, bits: u32) -> i32 {
        self.read(bits) - (1 << (bits - 1))
    }

    /// Moves past `bits` bits.
    pub(crate) fn skip(&mut self, bits: usize) {
        self.position += bits;
    }

    /// Moves to the next byte boundary, unless already on one.
    pub(crate) fn align_to_byte(&mut self) {
        self.position = self.position.next_multiple_of(8);
    }
}

/// `value`'s low `bits` bits read as a two's complement number.
pub(crate) fn sign_extend(value: i32, bits: u32) -> i32 {
    let shift = 32 - bits;
    (value << shift) >> shift
}

#[cfg(test)]
mod tests {
    use super::{BitReader, sign_extend};

    /// Bits come most significant first, across byte boundaries, and read as zero past the end,
    /// which marks the reader as overrun.
    #[test]
    fn bits_read_most_significant_first() {
        let mut reader = BitReader::new(&[0b1010_0000, 0b1111_0001]);
        assert_eq!(reader.read(3), 0b101);
        assert_eq!(reader.read(9), 0b0_0000_1111);
        assert_eq!(reader.read_signed(4), 1);
        assert!(!reader.overran());
        assert_eq!(reader.read(4), 0);
        assert!(reader.overran());
    }

    /// Two's complement and offset binary agree on what a field means.
    #[test]
    fn signed_fields_are_read_as_their_coding_says() {
        assert_eq!(sign_extend(0b111, 3), -1);
        assert_eq!(sign_extend(0b011, 3), 3);
        let mut reader = BitReader::new(&[0b0000_1000]);
        assert_eq!(reader.read_offset_binary(5), -15);
        reader.align_to_byte();
        assert_eq!(reader.position(), 8);
        reader.align_to_byte();
        assert_eq!(reader.position(), 8);
    }
}
