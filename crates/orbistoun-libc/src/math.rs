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

/// Everything here, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFloatFn)] {
    &[
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
