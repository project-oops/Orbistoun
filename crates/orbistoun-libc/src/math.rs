//! The maths library, which speaks entirely in floating-point registers.
//!
//! # Why these could not work before
//!
//! A `double` argument travels in `xmm0`-`xmm7` and never in the integer registers, and
//! the call boundary carried only integers. So the guest put 4.0 in `xmm0`, the handler
//! read six integer registers that did not contain it, and answered in `rax` - which the
//! guest was not reading. `sqrt(4)` came back as **4**: the guest's own argument, still in
//! `xmm0` because nothing had written it.
//!
//! Thirteen conformance checks failed that way, and every one of them was the same gap
//! wearing a different hat (D268).
//!
//! # Why the host's own maths is the right answer here
//!
//! Both sides are IEEE 754 doubles on the same architecture, and these functions are
//! defined by that standard rather than by the platform. `sqrt` is correctly rounded by
//! the specification, so there is one right answer and the host produces it.
//!
//! The transcendentals - `sin`, `cos`, `exp`, `log`, `pow` - are **not** correctly rounded
//! by IEEE 754, and two conforming libraries may differ in the last bit. Recorded as an
//! assumption rather than glossed: a title comparing a computed value against a stored
//! constant could see that difference, and the probe on real hardware is what would settle
//! it.

use orbistoun_core::{GUEST_ARG_REGISTERS, GUEST_FLOAT_REGISTERS, GuestFloatFn};

/// Reads floating-point argument `n` as a `double`.
fn arg(floats: &[u64; GUEST_FLOAT_REGISTERS], n: usize) -> f64 {
    f64::from_bits(floats[n])
}

/// Reads floating-point argument `n` as a `float`.
///
/// The low half of the register, which is where a single-precision value sits - not a
/// narrowing of the double, which would round twice and disagree with the guest in the
/// last bit.
fn arg_f32(floats: &[u64; GUEST_FLOAT_REGISTERS], n: usize) -> f32 {
    f32::from_bits((floats[n] & 0xFFFF_FFFF) as u32)
}

/// The bits a `double` answer goes back in.
const fn ret(value: f64) -> u64 {
    value.to_bits()
}

/// The bits a `float` answer goes back in.
///
/// The upper half is left zero. A caller reads the low half and nothing defines the rest,
/// so writing anything there would be inventing a value.
const fn ret_f32(value: f32) -> u64 {
    value.to_bits() as u64
}

/// Builds a one-argument `double` function.
macro_rules! unary {
    ($name:ident, $method:ident) => {
        fn $name(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
            ret(arg(floats, 0).$method())
        }
    };
}

/// Builds a two-argument `double` function.
macro_rules! binary {
    ($name:ident, $method:ident) => {
        fn $name(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
            ret(arg(floats, 0).$method(arg(floats, 1)))
        }
    };
}

/// Builds a one-argument `float` function.
macro_rules! unary_f32 {
    ($name:ident, $method:ident) => {
        fn $name(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
            ret_f32(arg_f32(floats, 0).$method())
        }
    };
}

/// Builds a two-argument `float` function.
macro_rules! binary_f32 {
    ($name:ident, $method:ident) => {
        fn $name(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
            ret_f32(arg_f32(floats, 0).$method(arg_f32(floats, 1)))
        }
    };
}

unary!(sqrt, sqrt);
unary!(fabs, abs);
unary!(floor, floor);
unary!(ceil, ceil);
unary!(sin, sin);
unary!(cos, cos);
unary!(tan, tan);
unary!(exp, exp);
unary!(trunc, trunc);
binary!(pow, powf);
unary!(log2, log2);
unary!(asin, asin);
unary!(acos, acos);
unary!(atan, atan);
binary!(atan2, atan2);

// The single-precision family. Each computes in `f32` rather than narrowing an `f64`
// result, which would round twice and disagree with the guest in the last bit.
unary_f32!(floorf, floor);
unary_f32!(ceilf, ceil);
unary_f32!(truncf, trunc);
unary_f32!(roundf, round);
unary_f32!(sinf, sin);
unary_f32!(cosf, cos);
unary_f32!(tanf, tan);
unary_f32!(expf, exp);
binary_f32!(powf, powf);

/// `logf(x)` - single-precision natural logarithm.
fn logf(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret_f32(arg_f32(floats, 0).ln())
}

/// `fmodf(x, y)`.
fn fmodf(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret_f32(arg_f32(floats, 0) % arg_f32(floats, 1))
}

/// `strtod(text, end)` - the one here that reads an integer register and answers in `xmm0`.
///
/// The pointer arrives in `rdi` and the result leaves in `xmm0`, which is why the two
/// argument arrays are carried together rather than a function being one kind or the other.
///
/// **`end` is written when it is supplied**, because a caller uses it to walk a list of
/// numbers and a loop that never advances is a hang rather than a wrong value. Rust's
/// parser is stricter than C's - it will not accept a trailing suffix - so the longest
/// parsable prefix is found rather than handing the whole string over and failing.
fn strtod(ints: &[u64; GUEST_ARG_REGISTERS], _floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    let Some(text) = crate::read_guest_path(ints[0]) else {
        return ret(0.0);
    };
    let trimmed = text.trim_start();
    let skipped = text.len() - trimmed.len();
    // Longest prefix that parses, which is what C specifies and Rust's `parse` does not.
    let mut best: Option<(usize, f64)> = None;
    for end in 1..=trimmed.len() {
        if let Ok(value) = trimmed[..end].parse::<f64>() {
            best = Some((end, value));
        }
    }
    // **When nothing converts, the end pointer is the original string.** The skipped
    // whitespace does not count as consumed: a caller walking a list tells "no number
    // here" from "a number I have passed" by whether the pointer moved at all, and
    // leading space on its own is not a number. The integer family already answers this
    // way, and the two disagreeing would be worse than either rule.
    let (consumed, value) = best.map_or((0, 0.0), |(end, value)| (skipped + end, value));
    if ints[1] != 0 {
        if let Ok(at) = usize::try_from(ints[1]) {
            let end_pointer = ints[0].saturating_add(consumed as u64);
            // SAFETY: a guest-supplied `char **` under the identity mapping (D014), written
            // only when the guest asked for it by passing a non-null pointer.
            unsafe {
                std::ptr::write_unaligned(
                    std::ptr::with_exposed_provenance_mut::<u64>(at),
                    end_pointer,
                );
            }
        }
    }
    ret(value)
}

/// `fmod(x, y)` - the remainder with the sign of `x`.
///
/// Rust's `%` on floats is the C `fmod`, not a Euclidean remainder, which is what the
/// standard asks for here.
fn fmod(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret(arg(floats, 0) % arg(floats, 1))
}

/// `log(x)` - the natural logarithm.
///
/// Named `ln` in Rust; `log` in Rust takes a base and would answer a different question
/// entirely from the same call.
fn log(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret(arg(floats, 0).ln())
}

/// `log10(x)`.
fn log10(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret(arg(floats, 0).log10())
}

/// `round(x)` - halfway cases away from zero.
///
/// **Not `rint` and not banker's rounding.** The standard specifies away-from-zero for this
/// function, so `round(2.5)` is 3, and the conformance probe checks exactly that. Rust's
/// `f64::round` has the same rule.
fn round(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret(arg(floats, 0).round())
}

/// `sqrtf(x)` - single precision.
fn sqrtf(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret_f32(arg_f32(floats, 0).sqrt())
}

/// `fabsf(x)` - single precision.
fn fabsf(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret_f32(arg_f32(floats, 0).abs())
}

binary_f32!(atan2f, atan2);

/// `sincosf(x, sinp, cosp)` - sine and cosine of `x` at once, each written through its pointer.
///
/// The angle arrives in `xmm0`; the two destinations are pointers in the integer registers (`rdi`,
/// `rsi`), so this reads both argument arrays - the same shape as [`strtod`]. Computed in `f32`
/// rather than narrowed from an `f64`, so it agrees with the guest in the last bit, and each result
/// is written only when its pointer is non-null, since a caller may want just one.
fn sincosf(ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    let (sin, cos) = arg_f32(floats, 0).sin_cos();
    for (pointer, value) in [(ints[0], sin), (ints[1], cos)] {
        if pointer == 0 {
            continue;
        }
        let Ok(at) = usize::try_from(pointer) else {
            continue;
        };
        // SAFETY: a guest-supplied `float *` under the identity mapping (D014), written only when
        // the guest passed a non-null pointer for it.
        unsafe {
            std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<f32>(at), value);
        }
    }
    0
}

/// `strtof(text, end)` - single precision.
///
/// Parsed as an `f32` rather than narrowed from the `f64` [`strtod`] produces: narrowing
/// rounds twice, and a value exactly between two `f32`s would land on the wrong one.
fn strtof(ints: &[u64; GUEST_ARG_REGISTERS], _floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    let Some(text) = crate::read_guest_path(ints[0]) else {
        return ret_f32(0.0);
    };
    let trimmed = text.trim_start();
    let skipped = text.len() - trimmed.len();
    let mut best: Option<(usize, f32)> = None;
    for end in 1..=trimmed.len() {
        if let Ok(value) = trimmed[..end].parse::<f32>() {
            best = Some((end, value));
        }
    }
    // **When nothing converts, the end pointer is the original string.** The skipped
    // whitespace does not count as consumed: a caller walking a list tells "no number
    // here" from "a number I have passed" by whether the pointer moved at all, and
    // leading space on its own is not a number. The integer family already answers this
    // way, and the two disagreeing would be worse than either rule.
    let (consumed, value) = best.map_or((0, 0.0), |(end, value)| (skipped + end, value));
    if ints[1] != 0 {
        if let Ok(at) = usize::try_from(ints[1]) {
            let end_pointer = ints[0].saturating_add(consumed as u64);
            // SAFETY: a guest-supplied `char **` under the identity mapping (D014), written
            // only when the guest asked for it by passing a non-null pointer.
            unsafe {
                std::ptr::write_unaligned(
                    std::ptr::with_exposed_provenance_mut::<u64>(at),
                    end_pointer,
                );
            }
        }
    }
    ret_f32(value)
}

// ---------------------------------------------------------------------------------------
// Added in bulk from ISO/IEC 9899 7.12, ahead of any guest reaching them (D472). The simple
// ones are the standard's function under Rust's name for it; the ones written out by hand
// below are the ones where that correspondence does not exist.
//
// **Each line names the standard, not just the clause.** These once read `- 7.12.4.2.` and
// leaned on this header for the rest, which is fine for a person and wrong for the knowledge
// generator: it takes a citation from the line it is describing, so a bare clause number
// became the entry's *purpose* and the entry was recorded `assumed` with "cites no published
// specification". `acosf` below, alone in carrying the full reference, was recorded
// `published`. One block, one generator, and the only difference was six characters.

// `acosf(x)` - ISO/IEC 9899 7.12.4.1.
unary_f32!(acosf, acos);
// `asinf(x)` - ISO/IEC 9899 7.12.4.2.
unary_f32!(asinf, asin);
// `atanf(x)` - ISO/IEC 9899 7.12.4.3.
unary_f32!(atanf, atan);
// `cbrtf(x)` - ISO/IEC 9899 7.12.7.1.
unary_f32!(cbrtf, cbrt);
// `exp2f(x)` - ISO/IEC 9899 7.12.6.4.
unary_f32!(exp2f, exp2);
// `log10f(x)` - ISO/IEC 9899 7.12.6.8.
unary_f32!(log10f, log10);
// `log2f(x)` - ISO/IEC 9899 7.12.6.10.
unary_f32!(log2f, log2);
// `tanhf(x)` - ISO/IEC 9899 7.12.5.6.
unary_f32!(tanhf, tanh);
// `hypotf(x, y)` - ISO/IEC 9899 7.12.7.3.
// Computed without the overflow a naive `sqrt(x*x + y*y)` has.
binary_f32!(hypotf, hypot);
// `exp2(x)` - ISO/IEC 9899 7.12.6.4.
// Double precision, beside `exp2f` above.
unary!(exp2, exp2);

/// `nearbyintf(x)` - ISO/IEC 9899 7.12.9.3.
///
/// Rounds to the nearest integer **in the current rounding direction, without raising the
/// inexact exception**. The default direction is to-nearest-ties-to-even, which is what this
/// answers; nothing here changes the rounding mode, so there is no other direction to honour.
/// Not `round`, which is ties-away-from-zero and would disagree on exactly the halfway cases.
fn nearbyintf(_ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret_f32(arg_f32(floats, 0).round_ties_even())
}

/// Multiplies by a power of two, without the intermediate overflowing.
///
/// `2^n` is exactly representable only for `n` in the exponent's range, so a single
/// `x * 2f64.powi(n)` answers infinity for a large `n` even when the product would be finite.
/// Splitting the scaling keeps every factor representable, and multiplying by an exact power
/// of two is itself exact - so the answer is correctly rounded once, at the end.
fn scale(mut x: f64, mut n: i32) -> f64 {
    /// The largest and smallest exponents a `double` can hold as a normal number.
    const HIGH: i32 = 1023;
    const LOW: i32 = -1022;
    while n > HIGH {
        x *= 2.0_f64.powi(HIGH);
        n -= HIGH;
    }
    while n < LOW {
        x *= 2.0_f64.powi(LOW);
        n -= LOW;
    }
    x * 2.0_f64.powi(n)
}

/// `ldexp(x, n)` - `x` times two to the `n`. ISO/IEC 9899 7.12.6.6.
///
/// **`n` arrives in an integer register**, not a floating-point one: it is an `int`, and
/// reading it from `floats` would read whatever the caller last put there.
fn ldexp(ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret(scale(arg(floats, 0), ints[0] as i32))
}

/// `ldexpf(x, n)` - ISO/IEC 9899 7.12.6.6.
///
/// Single precision, beside `ldexp`.
fn ldexpf(ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    ret_f32(scale(f64::from(arg_f32(floats, 0)), ints[0] as i32) as f32)
}

/// Writes a `double` into guest memory, if the caller gave somewhere to put it.
fn write_f64(at: u64, value: f64) {
    if at == 0 {
        return;
    }
    // SAFETY: a guest-supplied out-parameter under the identity mapping (D014); eight bytes,
    // which is what the `double *` the caller declared points at.
    unsafe { std::ptr::write_unaligned(crate::ptr(at).cast::<f64>(), value) };
}

/// Writes a `float` into guest memory, if the caller gave somewhere to put it.
fn write_f32(at: u64, value: f32) {
    if at == 0 {
        return;
    }
    // SAFETY: as above, four bytes for the `float *` the caller declared.
    unsafe { std::ptr::write_unaligned(crate::ptr(at).cast::<f32>(), value) };
}

/// `frexp(x, exp)` - splits `x` into a fraction in `[0.5, 1)` and a power of two.
///
/// ISO/IEC 9899 7.12.6.4. **Zero, infinity and NaN are answered unchanged with `*exp` set to
/// zero**, which the standard requires and which a formula-based version gets wrong: taking a
/// logarithm of zero would answer negative infinity and store nonsense.
fn frexp(ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    let x = arg(floats, 0);
    let out = ints[0];
    if x == 0.0 || !x.is_finite() {
        write_exponent(out, 0);
        return ret(x);
    }
    // The exponent such that scaling by it lands the magnitude in [0.5, 1).
    let mut exponent = 0_i32;
    let mut fraction = x;
    while fraction.abs() >= 1.0 {
        fraction /= 2.0;
        exponent += 1;
    }
    while fraction.abs() < 0.5 {
        fraction *= 2.0;
        exponent -= 1;
    }
    write_exponent(out, exponent);
    ret(fraction)
}

/// Writes `frexp`'s `int` out-parameter.
fn write_exponent(at: u64, value: i32) {
    if at == 0 {
        return;
    }
    // SAFETY: a guest-supplied `int *` under the identity mapping (D014); four bytes.
    unsafe { std::ptr::write_unaligned(crate::ptr(at).cast::<i32>(), value) };
}

/// `modf(x, iptr)` - splits `x` into its fractional and integral parts.
///
/// ISO/IEC 9899 7.12.6.12. **Both parts carry `x`'s sign**, so `modf(-3.5)` answers `-0.5`
/// and stores `-3.0` - not `0.5` and `-4.0`, which is what flooring would give.
fn modf(ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    let x = arg(floats, 0);
    let integral = x.trunc();
    write_f64(ints[0], integral);
    ret(if x.is_infinite() {
        0.0_f64.copysign(x)
    } else {
        x - integral
    })
}

/// `modff(x, iptr)` - ISO/IEC 9899 7.12.6.12.
///
/// Single precision, beside `modf`.
fn modff(ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    let x = arg_f32(floats, 0);
    let integral = x.trunc();
    write_f32(ints[0], integral);
    ret_f32(if x.is_infinite() {
        0.0_f32.copysign(x)
    } else {
        x - integral
    })
}

/// `sincos(x, s, c)` - both at once, into two out-parameters.
///
/// Reference: FreeBSD `sincos(3)`. Not ISO C - it is a BSD extension, and the target's C
/// library is FreeBSD-derived, which is why it is imported at all. Answers nothing.
fn sincos(ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    let x = arg(floats, 0);
    write_f64(ints[0], x.sin());
    write_f64(ints[1], x.cos());
    0
}

/// Everything here, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFloatFn)] {
    &[
        ("acosf", acosf),
        ("asinf", asinf),
        ("atanf", atanf),
        ("cbrtf", cbrtf),
        ("exp2", exp2),
        ("exp2f", exp2f),
        ("frexp", frexp),
        ("hypotf", hypotf),
        ("ldexp", ldexp),
        ("ldexpf", ldexpf),
        ("log10f", log10f),
        ("log2f", log2f),
        ("modf", modf),
        ("modff", modff),
        ("nearbyintf", nearbyintf),
        ("sincos", sincos),
        ("tanhf", tanhf),
        ("sqrt", sqrt),
        ("sqrtf", sqrtf),
        ("fabs", fabs),
        ("fabsf", fabsf),
        ("floor", floor),
        ("ceil", ceil),
        ("trunc", trunc),
        ("round", round),
        ("fmod", fmod),
        ("pow", pow),
        ("sin", sin),
        ("cos", cos),
        ("tan", tan),
        ("exp", exp),
        ("log", log),
        ("log10", log10),
        ("log2", log2),
        ("asin", asin),
        ("acos", acos),
        ("atan", atan),
        ("atan2", atan2),
        ("atan2f", atan2f),
        ("sincosf", sincosf),
        ("floorf", floorf),
        ("ceilf", ceilf),
        ("truncf", truncf),
        ("roundf", roundf),
        ("fmodf", fmodf),
        ("powf", powf),
        ("sinf", sinf),
        ("cosf", cosf),
        ("tanf", tanf),
        ("expf", expf),
        ("logf", logf),
        ("strtod", strtod),
        ("strtof", strtof),
    ]
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "exactness is the property under test - that scaling by a power of two, a               ties-to-even round and a sign-preserving split are exact rather than close.               An epsilon here would let the wrong answer pass, which is the whole failure               these guard against"
)]
mod bulk_ported {
    use super::{
        GUEST_ARG_REGISTERS, GUEST_FLOAT_REGISTERS, frexp, ldexp, modf, nearbyintf, scale,
    };

    fn call(
        f: fn(&[u64; GUEST_ARG_REGISTERS], &[u64; GUEST_FLOAT_REGISTERS]) -> u64,
        ints: [u64; 2],
        x: f64,
    ) -> u64 {
        let mut i = [0_u64; GUEST_ARG_REGISTERS];
        i[..2].copy_from_slice(&ints);
        let mut d = [0_u64; GUEST_FLOAT_REGISTERS];
        d[0] = x.to_bits();
        f(&i, &d)
    }

    /// **The cases a formula gets wrong.** Zero and infinity must come back unchanged with the
    /// exponent set to zero; a version taking a logarithm answers negative infinity for zero
    /// and stores nonsense in the caller's `int`.
    #[test]
    fn frexp_answers_zero_and_infinity_unchanged() {
        let mut out = 99_i32;
        let at = std::ptr::from_mut(&mut out) as u64;
        assert_eq!(f64::from_bits(call(frexp, [at, 0], 0.0)), 0.0);
        assert_eq!(out, 0, "the exponent is zeroed, not left as it was");
        out = 99;
        assert!(f64::from_bits(call(frexp, [at, 0], f64::INFINITY)).is_infinite());
        assert_eq!(out, 0);
    }

    /// And the ordinary case holds its own contract: the fraction is in `[0.5, 1)` and
    /// multiplying it back by the power of two gives the original.
    #[test]
    fn frexp_splits_into_a_fraction_and_a_power_of_two() {
        let mut out = 0_i32;
        let at = std::ptr::from_mut(&mut out) as u64;
        for x in [1.0_f64, 3.5, -12.25, 0.125, 1e300, 1e-300] {
            let m = f64::from_bits(call(frexp, [at, 0], x));
            assert!(
                (0.5..1.0).contains(&m.abs()),
                "{x}: fraction {m} is outside [0.5, 1)"
            );
            assert_eq!(m * 2.0_f64.powi(out), x, "{x}: does not reassemble");
        }
    }

    /// **Both parts carry the sign.** `modf(-3.5)` is `-0.5` and `-3.0`; flooring would give
    /// `0.5` and `-4.0`, which reassembles to the same number and is still wrong.
    #[test]
    fn modf_gives_both_parts_the_sign_of_the_argument() {
        let mut integral = 0.0_f64;
        let at = std::ptr::from_mut(&mut integral) as u64;
        let fraction = f64::from_bits(call(modf, [at, 0], -3.5));
        assert_eq!(fraction, -0.5);
        assert_eq!(integral, -3.0);
    }

    /// `ldexp` must stay exact past the point where a single `2^n` would overflow, and must
    /// still reach the smallest subnormal going the other way.
    #[test]
    fn ldexp_scales_beyond_a_single_power_of_two() {
        assert_eq!(
            scale(1.0, 1023),
            2.0_f64.powi(1023),
            "the largest normal power"
        );
        assert!(scale(1.0, 1024).is_infinite(), "and past it is infinity");
        assert_eq!(
            scale(1.0, -1074),
            f64::from_bits(1),
            "the smallest subnormal, which needs the split to reach"
        );
        assert_eq!(f64::from_bits(call(ldexp, [3, 0], 1.0)), 8.0);
    }

    /// **Ties to even, not away from zero.** `round` would answer 1 and 3 here, and the
    /// difference is invisible except on exactly the halfway cases.
    #[test]
    fn nearbyintf_rounds_halves_to_even() {
        let f = |x: f32| {
            let i = [0_u64; GUEST_ARG_REGISTERS];
            let mut d = [0_u64; GUEST_FLOAT_REGISTERS];
            d[0] = u64::from(x.to_bits());
            f32::from_bits((nearbyintf(&i, &d) & 0xFFFF_FFFF) as u32)
        };
        assert_eq!(f(0.5), 0.0);
        assert_eq!(f(1.5), 2.0);
        assert_eq!(f(2.5), 2.0);
        assert_eq!(f(-0.5), 0.0);
    }
}
