//! IEEE 754 half floats as a `COLOR_16` `FLOAT` target holds them, and the 32-bit floats its
//! `R32_SFLOAT` attachment draws in (D868).
//!
//! A half widens to a float exactly. A float narrows to the nearest half, a tie to the even one,
//! overflow to infinity and underflow through the subnormals to zero, with sign kept; a NaN stays a
//! NaN, quiet.

/// The 32-bit float bits that `half` holds exactly.
pub(crate) const fn widened(half: u16) -> u32 {
    let sign = ((half as u32) & 0x8000) << 16;
    let exponent = ((half >> 10) & 0x1F) as u32;
    let mantissa = (half & 0x3FF) as u32;
    match exponent {
        0 if mantissa == 0 => sign,
        // A subnormal: shifted until its leading one is the implicit bit, the exponent counting
        // down from the smallest normal's.
        0 => {
            let mut mantissa = mantissa;
            let mut exponent: u32 = 127 - 15 + 1;
            while mantissa & 0x400 == 0 {
                mantissa <<= 1;
                exponent -= 1;
            }
            sign | (exponent << 23) | ((mantissa & 0x3FF) << 13)
        }
        // Infinity, or a NaN with its payload.
        0x1F => sign | 0x7F80_0000 | (mantissa << 13),
        _ => sign | ((exponent + 127 - 15) << 23) | (mantissa << 13),
    }
}

/// The half nearest the 32-bit float `bits`, a tie to the even one.
pub(crate) const fn narrowed(bits: u32) -> u16 {
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let mantissa = bits & 0x7F_FFFF;
    if exponent == 0xFF {
        // A NaN keeps its top payload bits and is quiet; infinity stays infinity.
        let nan = if mantissa == 0 {
            0
        } else {
            0x200 | (mantissa >> 13) as u16
        };
        return sign | 0x7C00 | nan;
    }
    let half_exponent = exponent - 127 + 15;
    if half_exponent >= 0x1F {
        return sign | 0x7C00;
    }
    if half_exponent <= 0 {
        // Below the smallest normal: the value in units of the smallest subnormal, 2^-24, is the
        // significand with its implicit bit shifted right by 14 less the exponent.
        if half_exponent < -10 {
            return sign;
        }
        let significand = mantissa | 0x80_0000;
        let shift = (14 - half_exponent) as u32;
        return sign | round_shifted(significand, shift) as u16;
    }
    // A carry out of the mantissa steps the exponent, and from the largest finite to infinity.
    sign | ((half_exponent as u32) << 10 | round_shifted(mantissa, 13)) as u16
}

/// `value >> shift`, rounded to the nearest, a tie to the even.
const fn round_shifted(value: u32, shift: u32) -> u32 {
    let kept = value >> shift;
    let rest = value & ((1 << shift) - 1);
    let half_way = 1 << (shift - 1);
    if rest > half_way || (rest == half_way && kept & 1 == 1) {
        kept + 1
    } else {
        kept
    }
}

#[cfg(test)]
mod tests {
    use super::{narrowed, widened};

    /// Every half widens to a float and narrows back to itself: each is a float exactly, so the
    /// nearest half to it is it. A NaN comes back a NaN.
    #[test]
    fn every_half_survives_the_round_trip() {
        for half in 0..=u16::MAX {
            let back = narrowed(widened(half));
            let nan = half & 0x7C00 == 0x7C00 && half & 0x3FF != 0;
            if nan {
                assert!(back & 0x7C00 == 0x7C00 && back & 0x3FF != 0, "{half:#06x}");
            } else {
                assert_eq!(back, half, "{half:#06x}");
            }
        }
    }

    /// A half widens to the float it means: one, the smallest subnormal 2^-24, the largest finite
    /// 65504, negative two and infinity.
    #[test]
    fn a_half_widens_to_its_value() {
        assert_eq!(widened(0x3C00), 1.0_f32.to_bits());
        assert_eq!(widened(0x0001), 2.0_f32.powi(-24).to_bits());
        assert_eq!(widened(0x7BFF), 65504.0_f32.to_bits());
        assert_eq!(widened(0xC000), (-2.0_f32).to_bits());
        assert_eq!(widened(0x7C00), f32::INFINITY.to_bits());
    }

    /// A float between two halves narrows to the nearer, a tie to the even: one and a half units
    /// past one rounds up, half a unit stays at one; 65520, half way past the largest finite,
    /// overflows; 2^-25, half the smallest subnormal, is zero, and a little more is that subnormal.
    #[test]
    fn a_float_narrows_to_the_nearest_half() {
        let unit = 2.0_f32.powi(-10);
        assert_eq!(narrowed((1.0 + unit * 1.5).to_bits()), 0x3C02);
        assert_eq!(narrowed((1.0 + unit * 0.5).to_bits()), 0x3C00);
        assert_eq!(narrowed(65520.0_f32.to_bits()), 0x7C00);
        assert_eq!(narrowed(2.0_f32.powi(-25).to_bits()), 0x0000);
        assert_eq!(narrowed((2.0_f32.powi(-25) * 1.5).to_bits()), 0x0001);
        assert_eq!(narrowed((-0.0_f32).to_bits()), 0x8000);
    }
}
