//! The maths library, exercised across the floating-point call boundary.
//!
//! What is pinned is the boundary (D268): the argument is read from the floating-point
//! registers and the answer written back at the declared width. Inputs are chosen so the
//! answer differs from the input, since an echo passes any fixed-point test. Single-precision
//! functions compute in `f32` and answer in the low half with the upper half zero.

// Exact comparison is deliberate: `sqrt` is correctly rounded by IEEE 754 and the rounding
// functions are exact, so a tolerance would hide a wrong answer. The transcendentals use
// `near`.
#![allow(clippy::float_cmp)]

use orbistoun_core::{GUEST_ARG_REGISTERS, GUEST_FLOAT_REGISTERS, GuestFloatFn};

/// The implementation registered under `name`.
///
/// Panics on a missing name, which is a function no guest can reach.
fn implementation(name: &str) -> GuestFloatFn {
    orbistoun_libc::math::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(
            || panic!("{name} is not in the maths table, so nothing can call it"),
            |(_, f)| *f,
        )
}

/// Poison for registers the call does not use: a large negative `double` and an invalid
/// address, so reading the wrong register is obvious.
const POISON: u64 = 0xDEAD_BEEF_DEAD_BEEF;

/// Calls a `double` function with the given floating-point arguments.
fn call_f64(name: &str, args: &[f64]) -> f64 {
    let mut floats = [POISON; GUEST_FLOAT_REGISTERS];
    for (slot, value) in floats.iter_mut().zip(args) {
        *slot = value.to_bits();
    }
    f64::from_bits(implementation(name)(
        &[POISON; GUEST_ARG_REGISTERS],
        &floats,
    ))
}

/// Calls a `float` function, returning the raw register so the upper half stays visible.
fn call_f32_raw(name: &str, args: &[f32]) -> u64 {
    let mut floats = [POISON; GUEST_FLOAT_REGISTERS];
    for (slot, value) in floats.iter_mut().zip(args) {
        // A single-precision argument occupies the low half; the upper half is poison, which a
        // handler must not read.
        *slot = u64::from(value.to_bits()) | 0xFFFF_FFFF_0000_0000;
    }
    implementation(name)(&[POISON; GUEST_ARG_REGISTERS], &floats)
}

/// Calls a `float` function and reads the answer.
fn call_f32(name: &str, args: &[f32]) -> f32 {
    f32::from_bits((call_f32_raw(name, args) & 0xFFFF_FFFF) as u32)
}

/// Close enough for a transcendental, which IEEE 754 does not require to be correctly
/// rounded.
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * b.abs().max(1.0)
}

// The table.

/// Every name appears once, and the table is not empty.
#[test]
fn the_table_names_each_function_once() {
    let mut seen = std::collections::BTreeSet::new();
    for (name, _) in orbistoun_libc::math::implementations() {
        assert!(seen.insert(*name), "{name} appears in the table twice");
    }
    assert!(
        !seen.is_empty(),
        "the table is empty, so this file proves nothing"
    );
}

// The boundary itself.

/// No function answers with its own argument (D268).
#[test]
fn no_function_answers_with_the_argument_it_was_given() {
    for (name, input) in [
        ("sqrt", 4.0),
        ("fabs", -3.5),
        ("floor", 2.7),
        ("ceil", 2.3),
        ("trunc", 2.7),
        ("round", 2.5),
        ("sin", 1.0),
        ("cos", 1.0),
        ("tan", 1.0),
        ("exp", 1.0),
        ("log", 2.0),
        ("log10", 2.0),
        ("log2", 3.0),
        ("asin", 0.5),
        ("acos", 0.5),
        ("atan", 2.0),
    ] {
        let answer = call_f64(name, &[input]);
        assert!(
            (answer - input).abs() > 1e-9,
            "{name}({input}) answered {answer}, which is the argument it was handed"
        );
        assert!(answer.is_finite(), "{name}({input}) answered {answer}");
    }
}

/// The same, in single precision.
#[test]
fn no_single_precision_function_answers_with_its_argument() {
    for (name, input) in [
        ("sqrtf", 4.0_f32),
        ("fabsf", -3.5),
        ("floorf", 2.7),
        ("ceilf", 2.3),
        ("truncf", 2.7),
        ("roundf", 2.5),
        ("sinf", 1.0),
        ("cosf", 1.0),
        ("tanf", 1.0),
        ("expf", 1.0),
        ("logf", 2.0),
    ] {
        let answer = call_f32(name, &[input]);
        assert!(
            (answer - input).abs() > 1e-6,
            "{name}({input}) answered {answer}, which is the argument it was handed"
        );
        assert!(answer.is_finite(), "{name}({input}) answered {answer}");
    }
}

/// A two-argument function reads the second floating-point register, not a repeat of the
/// first.
#[test]
fn a_two_argument_function_reads_the_second_register() {
    assert!(near(call_f64("pow", &[2.0, 10.0]), 1024.0));
    assert!(near(
        call_f64("atan2", &[1.0, 1.0]),
        std::f64::consts::FRAC_PI_4
    ));
    assert!(near(call_f64("fmod", &[7.0, 3.0]), 1.0));

    // Asymmetric: reading `xmm0` twice answers 27 and reading `xmm1` twice answers 4.
    assert!(near(call_f64("pow", &[2.0, 3.0]), 8.0));
    assert!((call_f32("powf", &[2.0, 3.0]) - 8.0).abs() < 1e-5);
    assert!((call_f32("fmodf", &[7.0, 3.0]) - 1.0).abs() < 1e-6);
}

/// A single-precision answer leaves the upper half of the register zero.
#[test]
fn a_single_precision_answer_leaves_the_upper_half_alone() {
    for (name, input) in [("sqrtf", 4.0_f32), ("fabsf", -1.0), ("floorf", 1.5)] {
        let raw = call_f32_raw(name, &[input]);
        assert_eq!(
            raw >> 32,
            0,
            "{name} wrote {:#x} into the upper half of the result register",
            raw >> 32
        );
    }
}

/// A single-precision argument is read from the low half, ignoring whatever is above it.
#[test]
fn a_single_precision_argument_ignores_the_upper_half() {
    assert!((call_f32("sqrtf", &[9.0]) - 3.0).abs() < 1e-6);
    assert!((call_f32("fabsf", &[-2.5]) - 2.5).abs() < 1e-6);
}

// The answers the standard fixes.

/// `sqrt` is correctly rounded by IEEE 754, so it is asserted exactly.
#[test]
fn sqrt_is_exact_because_the_standard_requires_it() {
    assert_eq!(call_f64("sqrt", &[4.0]), 2.0);
    assert_eq!(call_f64("sqrt", &[0.0]), 0.0);
    assert_eq!(call_f64("sqrt", &[1.0]), 1.0);
    assert_eq!(call_f32("sqrtf", &[16.0]), 4.0);
}

/// `round` goes away from zero at a halfway case, as C specifies.
#[test]
fn round_takes_halfway_cases_away_from_zero() {
    assert_eq!(call_f64("round", &[2.5]), 3.0);
    assert_eq!(call_f64("round", &[3.5]), 4.0);
    assert_eq!(call_f64("round", &[-2.5]), -3.0);
    assert_eq!(call_f32("roundf", &[2.5]), 3.0);
    assert_eq!(call_f32("roundf", &[-2.5]), -3.0);
}

/// The four rounding functions answer four different things for -2.5, so none is wired to
/// another's implementation.
#[test]
fn the_rounding_functions_differ_on_a_negative() {
    assert_eq!(call_f64("floor", &[-2.5]), -3.0);
    assert_eq!(call_f64("ceil", &[-2.5]), -2.0);
    assert_eq!(call_f64("trunc", &[-2.5]), -2.0);
    assert_eq!(call_f64("round", &[-2.5]), -3.0);

    assert_eq!(call_f32("floorf", &[-2.5]), -3.0);
    assert_eq!(call_f32("ceilf", &[-2.5]), -2.0);
    assert_eq!(call_f32("truncf", &[-2.5]), -2.0);
}

/// `log` is the natural logarithm: `log(e) == 1`.
#[test]
fn log_is_the_natural_logarithm() {
    assert!(near(call_f64("log", &[std::f64::consts::E]), 1.0));
    assert!(near(call_f64("log10", &[1000.0]), 3.0));
    assert!(near(call_f64("log2", &[8.0]), 3.0));
    assert!((call_f32("logf", &[std::f32::consts::E]) - 1.0).abs() < 1e-6);

    // The three disagree on the same input, so none stands in for another.
    let x = 100.0;
    assert!(!near(call_f64("log", &[x]), call_f64("log10", &[x])));
    assert!(!near(call_f64("log2", &[x]), call_f64("log10", &[x])));
}

/// `fmod` keeps the sign of the dividend, which a Euclidean remainder does not.
#[test]
fn fmod_takes_the_sign_of_the_dividend() {
    assert_eq!(call_f64("fmod", &[-7.0, 3.0]), -1.0);
    assert_eq!(call_f64("fmod", &[7.0, -3.0]), 1.0);
    assert_eq!(call_f32("fmodf", &[-7.0, 3.0]), -1.0);
}

/// The inverse trigonometric functions answer in radians, and are each other's inverses.
#[test]
fn the_inverse_trigonometric_functions_answer_in_radians() {
    assert!(near(call_f64("asin", &[1.0]), std::f64::consts::FRAC_PI_2));
    assert!(near(call_f64("acos", &[1.0]), 0.0));
    assert!(near(call_f64("atan", &[1.0]), std::f64::consts::FRAC_PI_4));
    assert!(near(call_f64("sin", &[call_f64("asin", &[0.25])]), 0.25));
    assert!(near(call_f64("cos", &[call_f64("acos", &[0.25])]), 0.25));
}

/// `atan2` uses both signs to pick a quadrant.
#[test]
fn atan2_distinguishes_the_quadrants() {
    let quarter = std::f64::consts::FRAC_PI_4;
    assert!(near(call_f64("atan2", &[1.0, 1.0]), quarter));
    assert!(near(call_f64("atan2", &[1.0, -1.0]), 3.0 * quarter));
    assert!(near(call_f64("atan2", &[-1.0, -1.0]), -3.0 * quarter));
    assert!(near(call_f64("atan2", &[-1.0, 1.0]), -quarter));
}

/// The identities that tie the trigonometric family together, with individual values so a
/// swap of `sin` and `cos` is caught.
#[test]
fn the_trigonometric_functions_satisfy_their_identities() {
    let x = 0.7;
    let (s, c, t) = (
        call_f64("sin", &[x]),
        call_f64("cos", &[x]),
        call_f64("tan", &[x]),
    );
    assert!(near(s * s + c * c, 1.0));
    assert!(near(t, s / c));
    assert!(s < c, "sin(0.7) is below cos(0.7); a swap shows up here");
}

/// `exp` and `log` invert each other, and neither is the identity.
#[test]
fn exp_and_log_invert_each_other() {
    assert!(near(call_f64("exp", &[0.0]), 1.0));
    assert!(near(call_f64("log", &[call_f64("exp", &[2.5])]), 2.5));
    assert!(near(call_f64("exp", &[1.0]), std::f64::consts::E));
    assert!((call_f32("expf", &[0.0]) - 1.0).abs() < 1e-6);
}

/// `fabs` clears the sign and leaves everything else, including a negative zero's
/// magnitude.
#[test]
fn fabs_clears_only_the_sign() {
    assert_eq!(call_f64("fabs", &[-3.5]), 3.5);
    assert_eq!(call_f64("fabs", &[3.5]), 3.5);
    assert!(call_f64("fabs", &[-0.0]).is_sign_positive());
    assert_eq!(call_f32("fabsf", &[-3.5]), 3.5);
}

// The two that read an integer register.

/// A NUL-terminated string at a real address, which under the identity mapping is a guest
/// pointer.
struct Text(Vec<u8>);

impl Text {
    fn new(s: &str) -> Self {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        Self(v)
    }

    fn at(&self) -> u64 {
        self.0.as_ptr().expose_provenance() as u64
    }
}

/// Calls one of the two mixed functions: pointer in an integer register, answer in a
/// floating-point one.
fn call_mixed(name: &str, text: u64, end: u64) -> u64 {
    let mut ints = [POISON; GUEST_ARG_REGISTERS];
    ints[0] = text;
    ints[1] = end;
    implementation(name)(&ints, &[POISON; GUEST_FLOAT_REGISTERS])
}

/// `strtod` reads a pointer from an integer register and answers in a floating-point one.
#[test]
fn strtod_crosses_from_an_integer_register_to_a_floating_point_one() {
    let text = Text::new("2.5");
    assert_eq!(f64::from_bits(call_mixed("strtod", text.at(), 0)), 2.5);

    let single = Text::new("2.5");
    let raw = call_mixed("strtof", single.at(), 0);
    assert_eq!(f32::from_bits((raw & 0xFFFF_FFFF) as u32), 2.5);
    assert_eq!(raw >> 32, 0, "strtof wrote into the upper half");
}

/// The longest parsable prefix is taken, as C specifies.
#[test]
fn a_conversion_takes_the_longest_prefix_that_parses() {
    for (input, want) in [
        ("2.5abc", 2.5),
        ("  -1.25xyz", -1.25),
        ("3", 3.0),
        ("1e3;", 1000.0),
        ("0.5,0.25", 0.5),
    ] {
        let text = Text::new(input);
        assert_eq!(
            f64::from_bits(call_mixed("strtod", text.at(), 0)),
            want,
            "strtod({input:?})"
        );
    }
}

/// The end pointer lands past what was consumed, including skipped whitespace.
#[test]
fn a_conversion_reports_where_it_stopped() {
    let text = Text::new("  2.5,3.5");
    let mut end: u64 = 0;
    let slot = std::ptr::from_mut(&mut end).expose_provenance() as u64;

    assert_eq!(f64::from_bits(call_mixed("strtod", text.at(), slot)), 2.5);
    assert_eq!(
        end,
        text.at() + 5,
        "the end pointer should sit on the comma, past the two skipped spaces"
    );
}

/// Nothing parsable is zero, and the end pointer does not move.
#[test]
fn a_conversion_that_parses_nothing_consumes_nothing() {
    for input in ["abc", "", "   "] {
        let text = Text::new(input);
        let mut end: u64 = 0;
        let slot = std::ptr::from_mut(&mut end).expose_provenance() as u64;
        assert_eq!(
            f64::from_bits(call_mixed("strtod", text.at(), slot)),
            0.0,
            "strtod({input:?})"
        );
        assert_eq!(end, text.at(), "strtod({input:?}) moved the end pointer");
    }
}

/// A null pointer is zero rather than a fault, and a null end pointer is simply not
/// written.
#[test]
fn a_null_pointer_is_zero_rather_than_a_fault() {
    assert_eq!(f64::from_bits(call_mixed("strtod", 0, 0)), 0.0);
    assert_eq!(call_mixed("strtof", 0, 0), 0);

    let text = Text::new("6.5");
    assert_eq!(f64::from_bits(call_mixed("strtod", text.at(), 0)), 6.5);
}

/// The single-precision conversion parses as an `f32` rather than narrowing a `double`,
/// checked at a value where double rounding differs.
#[test]
fn the_single_precision_conversion_does_not_round_twice() {
    let text = Text::new("16777217"); // 2^24 + 1, the first integer an f32 cannot hold
    let raw = call_mixed("strtof", text.at(), 0);
    let answer = f32::from_bits((raw & 0xFFFF_FFFF) as u32);
    assert_eq!(answer, "16777217".parse::<f32>().expect("parses as f32"));
    assert_eq!(answer, 16_777_216.0);
}

/// The single-precision conversion reports where it stopped, as the double does.
#[test]
fn the_single_precision_conversion_also_reports_where_it_stopped() {
    let text = Text::new("  1.5e2;rest");
    let mut end: u64 = 0;
    let slot = std::ptr::from_mut(&mut end).expose_provenance() as u64;

    let raw = call_mixed("strtof", text.at(), slot);
    assert_eq!(f32::from_bits((raw & 0xFFFF_FFFF) as u32), 150.0);
    assert_eq!(end, text.at() + 7, "past the exponent, on the semicolon");

    // And nothing parsable leaves the pointer at the original string.
    let nothing = Text::new("  x");
    let mut stuck: u64 = 0;
    let stuck_slot = std::ptr::from_mut(&mut stuck).expose_provenance() as u64;
    assert_eq!(call_mixed("strtof", nothing.at(), stuck_slot), 0);
    assert_eq!(stuck, nothing.at());
}
