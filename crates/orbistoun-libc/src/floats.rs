//! The floating-point conversions of formatted output (D748).
//!
//! ISO C 7.21.6.1: `f` is `[-]ddd.ddd` with the precision's digits after the point, default six;
//! `e` is `[-]d.ddde±dd`, an exponent of at least two digits; `g` is whichever of the two the value's
//! exponent picks, with trailing zeros removed. Each rounds the exact binary value to the nearest
//! decimal, ties to even, as the default rounding mode does; Rust's formatting rounds the same way,
//! so its digits are used and only the spelling is C's.

/// The conversion of `value`, its sign included when negative and never otherwise: the `+` and space
/// flags, and padding, are the caller's, as they are for an integer.
///
/// `conversion` is one of `f F e E g G`; an upper-case one spells the exponent, the infinity and the
/// not-a-number upper-case. `alternate` is the `#` flag: always a decimal point, and for `g` the
/// trailing zeros kept.
pub(crate) fn render(
    value: f64,
    conversion: u8,
    precision: Option<usize>,
    alternate: bool,
) -> Vec<u8> {
    let upper = conversion.is_ascii_uppercase();
    let mut out = Vec::new();
    if value.is_sign_negative() {
        out.push(b'-');
    }
    let magnitude = value.abs();
    let body = if magnitude.is_nan() {
        b"nan".to_vec()
    } else if magnitude.is_infinite() {
        b"inf".to_vec()
    } else {
        match conversion.to_ascii_lowercase() {
            b'f' => fixed(magnitude, precision.unwrap_or(6), alternate),
            b'e' => exponent(magnitude, precision.unwrap_or(6), alternate),
            _ => general(magnitude, precision, alternate),
        }
    };
    out.extend(body);
    if upper {
        out.make_ascii_uppercase();
    }
    out
}

/// Whether a rendering is an infinity or a not-a-number, which pad with spaces whatever the `0`
/// flag says.
pub(crate) fn is_special(rendered: &[u8]) -> bool {
    rendered
        .iter()
        .any(|b| matches!(b.to_ascii_lowercase(), b'n' | b'i'))
}

/// `ddd.ddd`, with `precision` digits after the point.
fn fixed(magnitude: f64, precision: usize, alternate: bool) -> Vec<u8> {
    let mut text = format!("{magnitude:.precision$}").into_bytes();
    if precision == 0 && alternate {
        text.push(b'.');
    }
    text
}

/// `d.ddde±dd`, with `precision` digits after the point.
fn exponent(magnitude: f64, precision: usize, alternate: bool) -> Vec<u8> {
    let (mantissa, power) = split_exponent(magnitude, precision);
    let mut text = mantissa.into_bytes();
    if precision == 0 && alternate {
        text.push(b'.');
    }
    text.extend(exponent_suffix(power));
    text
}

/// The mantissa Rust renders at `precision` digits, and the power of ten it rounded to.
fn split_exponent(magnitude: f64, precision: usize) -> (String, i32) {
    let text = format!("{magnitude:.precision$e}");
    let (mantissa, power) = text.split_once('e').unwrap_or((&text, "0"));
    (mantissa.to_owned(), power.parse().unwrap_or(0))
}

/// `e+dd`: a sign always, and at least two digits.
fn exponent_suffix(power: i32) -> Vec<u8> {
    let sign = if power < 0 { '-' } else { '+' };
    format!("e{sign}{:02}", power.unsigned_abs()).into_bytes()
}

/// `g`: with `P` the precision (six when absent, one when zero) and `X` the exponent `e` style would
/// show at `P - 1` digits, `f` at `P - 1 - X` digits when `P > X >= -4`, otherwise `e` at `P - 1`;
/// then trailing zeros removed, and a point left with nothing after it, unless `alternate`.
fn general(magnitude: f64, precision: Option<usize>, alternate: bool) -> Vec<u8> {
    let significant = match precision {
        None => 6,
        Some(0) => 1,
        Some(p) => p,
    };
    let (_, power) = split_exponent(magnitude, significant - 1);
    let fits = i64::try_from(significant).unwrap_or(i64::MAX) > i64::from(power) && power >= -4;
    let (mut digits, suffix) = if fits {
        let after = usize::try_from(i64::try_from(significant).unwrap_or(0) - 1 - i64::from(power))
            .unwrap_or(0);
        (fixed(magnitude, after, alternate), Vec::new())
    } else {
        let (mantissa, power) = split_exponent(magnitude, significant - 1);
        let mut mantissa = mantissa.into_bytes();
        if significant == 1 && alternate {
            mantissa.push(b'.');
        }
        (mantissa, exponent_suffix(power))
    };
    if !alternate && digits.contains(&b'.') {
        while digits.last() == Some(&b'0') {
            digits.pop();
        }
        if digits.last() == Some(&b'.') {
            digits.pop();
        }
    }
    digits.extend(suffix);
    digits
}

#[cfg(test)]
mod tests {
    use super::render;

    fn r(value: f64, conversion: u8, precision: Option<usize>) -> String {
        String::from_utf8(render(value, conversion, precision, false)).expect("ascii")
    }

    /// The renderings ISO C's rules give, as any conforming library prints them.
    #[test]
    fn each_conversion_spells_the_value_as_c_does() {
        assert_eq!(r(12.345_678_9, b'f', None), "12.345679");
        assert_eq!(r(12.345_678_9, b'f', Some(2)), "12.35");
        assert_eq!(r(2.5, b'f', Some(0)), "2", "a tie rounds to even");
        assert_eq!(r(-0.0, b'f', Some(1)), "-0.0");
        assert_eq!(r(1234.5, b'e', None), "1.234500e+03");
        assert_eq!(r(0.000_123, b'E', Some(2)), "1.23E-04");
        assert_eq!(r(1e100, b'e', Some(0)), "1e+100");
        assert_eq!(r(100.0, b'g', None), "100");
        assert_eq!(r(0.0001, b'g', None), "0.0001");
        assert_eq!(r(0.000_01, b'g', None), "1e-05");
        assert_eq!(r(123_456_789.0, b'g', None), "1.23457e+08");
        assert_eq!(
            r(999_999.5, b'g', None),
            "1e+06",
            "rounding moves the exponent"
        );
        assert_eq!(r(0.5, b'g', Some(0)), "0.5");
        assert_eq!(r(1.0, b'G', Some(3)), "1");
        assert_eq!(r(0.0, b'g', None), "0");
        assert_eq!(r(f64::INFINITY, b'f', None), "inf");
        assert_eq!(r(f64::NEG_INFINITY, b'E', None), "-INF");
        assert_eq!(r(f64::NAN, b'g', None), "nan");
    }

    /// `#` keeps the point, and for `g` the trailing zeros.
    #[test]
    fn the_alternate_form_keeps_the_point() {
        let alt = |v: f64, c: u8, p: Option<usize>| {
            String::from_utf8(render(v, c, p, true)).expect("ascii")
        };
        assert_eq!(alt(2.0, b'f', Some(0)), "2.");
        assert_eq!(alt(2.0, b'e', Some(0)), "2.e+00");
        assert_eq!(alt(100.0, b'g', None), "100.000");
        assert_eq!(alt(1.0, b'g', Some(1)), "1.");
    }
}
