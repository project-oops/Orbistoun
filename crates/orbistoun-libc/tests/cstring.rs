//! The C string, character-class and integer-conversion functions, exercised as a guest
//! reaches them.
//!
//! # Why through the table, and not the functions
//!
//! Every function in `cstring` is private; only `implementations()` is public. That is the
//! right shape - the registry is the only thing that ever calls one - and it means a test
//! here reaches exactly what a resolved import reaches. A function dropped from the table
//! would fail these tests even while it still compiled, which is the same class of bug as a
//! module registered without its implementations (D281).
//!
//! # Guest addresses are host addresses
//!
//! The identity mapping (D014) means a guest pointer is a host pointer, so a `Vec<u8>` with
//! a terminator on the end is a valid C string as far as these functions are concerned.
//! `Text` holds the storage alive for the duration of a test and hands out its exposed
//! address.
//!
//! # What is deliberately not pinned here
//!
//! These are C standard functions, so the oracle is the standard rather than the platform,
//! and almost everything below asserts against it. Two behaviours are the implementation's
//! own choice inside undefined territory and are marked where they appear: `abs` of the
//! most negative value wraps, and a conversion that consumes no digits reports no
//! consumption. Both are stated in the source; the tests exist so a change to either is
//! visible rather than silent.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// A NUL-terminated byte string at a real address.
struct Text(Vec<u8>);

impl Text {
    /// From text. The bytes are the guest's, so nothing here assumes UTF-8 beyond the
    /// convenience of writing ASCII in a test.
    fn new(s: &str) -> Self {
        Self::raw(s.as_bytes())
    }

    /// From arbitrary bytes, for the cases that are not text.
    fn raw(b: &[u8]) -> Self {
        let mut v = b.to_vec();
        v.push(0);
        Self(v)
    }

    /// The address a guest would pass, with provenance exposed so the reads inside are
    /// sound rather than merely working.
    fn at(&self) -> u64 {
        self.0.as_ptr().expose_provenance() as u64
    }
}

/// A wide string, four bytes per character.
struct Wide(Vec<u32>);

impl Wide {
    fn new(chars: &[u32]) -> Self {
        let mut v = chars.to_vec();
        v.push(0);
        Self(v)
    }

    fn at(&self) -> u64 {
        self.0.as_ptr().expose_provenance() as u64
    }
}

/// The implementation registered under `name`.
///
/// Panics rather than returning an option: a name that is not in the table is a test asking
/// about a function no guest can reach, and reporting that as a skipped assertion would
/// hide it.
fn implementation(name: &str) -> GuestFn {
    orbistoun_libc::cstring::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(
            || panic!("{name} is not in the cstring table, so nothing can call it"),
            |(_, f)| *f,
        )
}

/// Calls one, filling the unused argument registers with a poison value.
///
/// The poison matters: a function that read an argument it does not take would otherwise
/// see a plausible zero and behave, and the mistake would only appear against a guest.
fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0xDEAD_BEEF_DEAD_BEEF_u64; GUEST_ARG_REGISTERS];
    for (slot, value) in regs.iter_mut().zip(args) {
        *slot = *value;
    }
    implementation(name)(&regs)
}

/// Calls a one-argument classification function on a byte.
fn classify(name: &str, b: u8) -> u64 {
    call(name, &[u64::from(b)])
}

// --- the table itself ---------------------------------------------------------------

/// Every name is distinct, and the table is not empty.
///
/// A duplicate would mean the registry silently picks one of two, and the pair could
/// disagree for a long time before anything noticed. The emptiness check is there because
/// every other test in this file would pass vacuously against an empty table - it would
/// simply never be reached, since `implementation` panics.
#[test]
fn the_table_names_each_function_once() {
    let mut seen = std::collections::BTreeSet::new();
    for (name, _) in orbistoun_libc::cstring::implementations() {
        assert!(seen.insert(*name), "{name} appears in the table twice");
    }
    assert!(
        !seen.is_empty(),
        "the table is empty, so this file proves nothing"
    );
}

// --- character classes --------------------------------------------------------------

/// Each class answers for the letters, digits and punctuation the C locale defines.
///
/// Asserted per class on a member and a non-member, because a predicate that answered
/// `true` for everything would pass a members-only test.
#[test]
fn the_character_classes_follow_the_c_locale() {
    for (name, member, outsider) in [
        ("isalpha", b'q', b'4'),
        ("isdigit", b'4', b'q'),
        ("isalnum", b'4', b'!'),
        ("isspace", b' ', b'q'),
        ("isupper", b'Q', b'q'),
        ("islower", b'q', b'Q'),
        ("ispunct", b'!', b'q'),
        ("isxdigit", b'e', b'g'),
        ("iscntrl", 0x07, b'q'),
        ("isprint", b' ', 0x07),
        ("isgraph", b'!', b' '),
    ] {
        assert_ne!(classify(name, member), 0, "{name} rejected {member:#04x}");
        assert_eq!(
            classify(name, outsider),
            0,
            "{name} accepted {outsider:#04x}"
        );
    }
}

/// `isspace` covers the vertical tab, which Rust's own `is_ascii_whitespace` leaves out.
///
/// The single place where the standard library's classification and C's disagree, so it is
/// the one worth a test of its own - a refactor that dropped the extra term would read as a
/// simplification.
#[test]
fn isspace_includes_the_vertical_tab() {
    for b in [b' ', b'\t', b'\n', 0x0b, 0x0c, b'\r'] {
        assert_ne!(classify("isspace", b), 0, "{b:#04x} is whitespace in C");
    }
    assert_eq!(classify("isspace", 0x00), 0);
}

/// The classes take an `int`, so a value that is not a byte belongs to none of them.
///
/// C requires the argument to be representable as `unsigned char` or equal to `EOF`, and a
/// table-driven implementation would index out of bounds on anything else. Here the answer
/// is simply "not a member", including for `EOF`, which is in no class.
#[test]
fn a_value_that_is_not_a_byte_is_in_no_class() {
    let eof = (-1_i64) as u64;
    for name in [
        "isalpha", "isdigit", "isspace", "isprint", "isgraph", "iscntrl",
    ] {
        assert_eq!(
            call(name, &[0x120]),
            0,
            "{name} classified a value above a byte"
        );
        assert_eq!(call(name, &[eof]), 0, "{name} classified EOF");
    }
}

/// Case conversion changes only the letters, and leaves everything else exactly as it came.
#[test]
fn case_conversion_touches_only_letters() {
    assert_eq!(call("toupper", &[u64::from(b'q')]), u64::from(b'Q'));
    assert_eq!(call("tolower", &[u64::from(b'Q')]), u64::from(b'q'));
    for b in *b"Q4! " {
        assert_eq!(
            call("toupper", &[u64::from(b)]),
            u64::from(b),
            "toupper changed {b:#04x}"
        );
    }
    for b in *b"q4! " {
        assert_eq!(
            call("tolower", &[u64::from(b)]),
            u64::from(b),
            "tolower changed {b:#04x}"
        );
    }
}

// --- searching ------------------------------------------------------------------------

/// `strstr` returns an address inside the haystack, not an index.
///
/// The distinction is the whole function: a caller does pointer arithmetic on the result,
/// so returning the offset would appear to work for a match at position zero and corrupt
/// every other case.
#[test]
fn strstr_returns_an_address_inside_the_haystack() {
    let haystack = Text::new("alpha beta gamma");
    let needle = Text::new("beta");
    assert_eq!(
        call("strstr", &[haystack.at(), needle.at()]),
        haystack.at() + 6
    );
}

/// An empty needle matches at the start, which the standard requires.
///
/// A windows-based search returns nothing for a zero-length window, so this is the case a
/// rewrite loses first.
#[test]
fn an_empty_needle_matches_at_the_start() {
    let haystack = Text::new("alpha");
    let needle = Text::new("");
    assert_eq!(call("strstr", &[haystack.at(), needle.at()]), haystack.at());
}

/// No match is null, and a needle longer than the haystack is no match rather than a panic.
#[test]
fn strstr_reports_absence_as_null() {
    let haystack = Text::new("alpha");
    for text in ["zeta", "alphabet", "alpha "] {
        let needle = Text::new(text);
        assert_eq!(
            call("strstr", &[haystack.at(), needle.at()]),
            0,
            "{text:?} is not in \"alpha\""
        );
    }
}

/// A match ending at the terminator still lands on its first byte, not past it.
#[test]
fn strstr_finds_a_match_at_the_end() {
    let haystack = Text::new("alpha beta");
    let needle = Text::new("beta");
    assert_eq!(
        call("strstr", &[haystack.at(), needle.at()]),
        haystack.at() + 6
    );
}

/// `strpbrk` finds the first byte belonging to a set, and reports its address.
#[test]
fn strpbrk_finds_the_first_byte_from_the_set() {
    let text = Text::new("alpha=beta;gamma");
    let accept = Text::new(";=");
    assert_eq!(call("strpbrk", &[text.at(), accept.at()]), text.at() + 5);

    let none = Text::new("#");
    assert_eq!(call("strpbrk", &[text.at(), none.at()]), 0);
}

/// `strspn` and `strcspn` are complements over the same input.
///
/// Asserted together because the pair is easy to write with one negation missing, and each
/// alone looks right.
#[test]
fn strspn_and_strcspn_are_complements() {
    let text = Text::new("   alpha");
    let spaces = Text::new(" ");
    assert_eq!(call("strspn", &[text.at(), spaces.at()]), 3);
    assert_eq!(call("strcspn", &[text.at(), spaces.at()]), 0);

    let letters = Text::new("abcdefghijklmnopqrstuvwxyz");
    assert_eq!(call("strspn", &[text.at(), letters.at()]), 0);
    assert_eq!(call("strcspn", &[text.at(), letters.at()]), 3);
}

/// A whole string matching, and an empty set, are the two ends of the range.
#[test]
fn a_span_can_cover_all_of_a_string_or_none_of_it() {
    let text = Text::new("aaaa");
    let a = Text::new("a");
    let empty = Text::new("");
    assert_eq!(call("strspn", &[text.at(), a.at()]), 4);
    assert_eq!(call("strspn", &[text.at(), empty.at()]), 0);
    assert_eq!(call("strcspn", &[text.at(), empty.at()]), 4);
}

/// Case-insensitive comparison reports a sign, and it is the sign of the folded compare.
#[test]
fn strcasecmp_ignores_ascii_case() {
    let upper = Text::new("ALPHA");
    let lower = Text::new("alpha");
    let later = Text::new("beta");

    assert_eq!(call("strcasecmp", &[upper.at(), lower.at()]), 0);
    assert_eq!(
        call("strcasecmp", &[lower.at(), later.at()]),
        (-1_i64) as u64
    );
    assert_eq!(call("strcasecmp", &[later.at(), upper.at()]), 1);
}

/// Folding is ASCII-only, so a high byte is not any letter's other case.
///
/// The locale trap: a case-folding routine that used a host locale would make this pair
/// equal on some machines and not on others.
#[test]
fn folding_does_not_reach_past_ascii() {
    let a = Text::raw(&[0xC0]);
    let b = Text::raw(&[0xE0]);
    assert_ne!(call("strcasecmp", &[a.at(), b.at()]), 0);
}

/// `strncasecmp` stops at `n`, and `n` of zero makes everything equal.
#[test]
fn strncasecmp_compares_at_most_n_bytes() {
    let a = Text::new("ALPHAbet");
    let b = Text::new("alphaBET");
    assert_eq!(call("strncasecmp", &[a.at(), b.at(), 5]), 0);

    let diverging = Text::new("alphaX");
    assert_eq!(call("strncasecmp", &[a.at(), diverging.at(), 5]), 0);
    assert_ne!(call("strncasecmp", &[a.at(), diverging.at(), 6]), 0);

    assert_eq!(call("strncasecmp", &[a.at(), diverging.at(), 0]), 0);
}

/// A limit beyond either string compares the whole of both.
///
/// The bound is applied to the bytes actually present, so an enormous `n` must not read
/// past a terminator looking for more to compare.
#[test]
fn a_limit_beyond_the_strings_compares_all_of_them() {
    let short = Text::new("a");
    let long = Text::new("ab");
    assert_eq!(
        call("strncasecmp", &[short.at(), long.at(), u64::MAX]),
        (-1_i64) as u64
    );
}

// --- integer conversion ---------------------------------------------------------------

/// `atoi` skips leading space, takes a sign, and stops at the first non-digit.
#[test]
fn atoi_reads_a_decimal_and_stops() {
    for (text, want) in [
        ("42", 42_i32),
        ("  42", 42),
        ("\t\n\x0b 42", 42),
        ("-42", -42),
        ("+42", 42),
        ("42abc", 42),
        ("42 43", 42),
    ] {
        let t = Text::new(text);
        assert_eq!(call("atoi", &[t.at()]) as i32, want, "atoi({text:?})");
    }
}

/// Nothing to read is zero, which is C's answer and not an error.
#[test]
fn atoi_of_something_that_is_not_a_number_is_zero() {
    for text in ["", "abc", "   ", "-", "+", "-abc"] {
        let t = Text::new(text);
        assert_eq!(call("atoi", &[t.at()]), 0, "atoi({text:?})");
    }
}

/// `atoi` is decimal even when the text looks octal, and `strtol` with base zero is not.
///
/// The one place the two families genuinely differ, and the reason the shared parser takes
/// a base rather than working it out unconditionally.
#[test]
fn atoi_is_always_decimal_but_base_zero_reads_the_prefix() {
    let octal = Text::new("010");
    assert_eq!(call("atoi", &[octal.at()]), 10);
    assert_eq!(call("strtol", &[octal.at(), 0, 0]), 8);

    let hex = Text::new("0x1f");
    assert_eq!(call("atoi", &[hex.at()]), 0);
    assert_eq!(call("strtol", &[hex.at(), 0, 0]), 31);

    let plain = Text::new("19");
    assert_eq!(call("strtol", &[plain.at(), 0, 0]), 19);
}

/// The `atoi` family differs only in the width it truncates to.
#[test]
fn the_atoi_family_truncates_to_its_own_width() {
    let big = Text::new("4294967298"); // 2^32 + 2
    assert_eq!(call("atoi", &[big.at()]) as i32, 2);
    assert_eq!(call("atol", &[big.at()]), 4_294_967_298);
    assert_eq!(call("atoll", &[big.at()]), 4_294_967_298);
}

/// Base 16 accepts the prefix but does not require it, which the standard demands.
#[test]
fn base_sixteen_accepts_the_prefix_without_needing_it() {
    let prefixed = Text::new("0xFF");
    let bare = Text::new("FF");
    assert_eq!(call("strtol", &[prefixed.at(), 0, 16]), 255);
    assert_eq!(call("strtol", &[bare.at(), 0, 16]), 255);
    assert_eq!(call("strtol", &[bare.at(), 0, 10]), 0);
}

/// An arbitrary base is honoured, and a digit outside it ends the number.
#[test]
fn an_arbitrary_base_stops_at_the_first_digit_outside_it() {
    let text = Text::new("zz");
    assert_eq!(call("strtol", &[text.at(), 0, 36]), 35 * 36 + 35);

    let binary = Text::new("10123");
    assert_eq!(call("strtol", &[binary.at(), 0, 2]), 0b101);
}

/// `strtol` writes where it stopped, so a caller can walk a list.
///
/// The failure this guards is a hang rather than a wrong answer: a loop that advances by
/// the reported end pointer never terminates if the pointer never moves.
#[test]
fn strtol_reports_where_it_stopped() {
    let text = Text::new("12,34");
    let mut end: u64 = 0;
    let slot = std::ptr::from_mut(&mut end).expose_provenance() as u64;

    assert_eq!(call("strtol", &[text.at(), slot, 10]), 12);
    assert_eq!(
        end,
        text.at() + 2,
        "the end pointer should sit on the comma"
    );
}

/// With no digits, nothing is consumed and the end pointer stays put.
///
/// Deliberate, and stated in the source: a caller's loop sees no progress and terminates,
/// rather than advancing past text it never converted.
#[test]
fn a_conversion_that_reads_no_digits_consumes_nothing() {
    for text in ["abc", "", "  ", "-"] {
        let t = Text::new(text);
        let mut end: u64 = 0;
        let slot = std::ptr::from_mut(&mut end).expose_provenance() as u64;
        assert_eq!(call("strtol", &[t.at(), slot, 10]), 0, "strtol({text:?})");
        assert_eq!(end, t.at(), "strtol({text:?}) moved the end pointer");
    }
}

/// A null `endptr` is not written, and is not a crash.
///
/// Guests pass null here constantly; the branch that skips the write is the one that keeps
/// the process alive.
#[test]
fn a_null_end_pointer_is_simply_not_written() {
    let text = Text::new("77");
    assert_eq!(call("strtol", &[text.at(), 0, 10]), 77);
}

/// The `strtol` family differs only in the width and signedness it reports.
///
/// `strtoul` of a negative wraps, which C specifies rather than leaves to an accident.
#[test]
fn the_strtol_family_differs_only_in_width_and_sign() {
    let negative = Text::new("-1");
    assert_eq!(call("strtol", &[negative.at(), 0, 10]) as i64, -1);
    assert_eq!(call("strtoll", &[negative.at(), 0, 10]) as i64, -1);
    assert_eq!(call("strtoul", &[negative.at(), 0, 10]), u64::MAX);
    assert_eq!(call("strtoull", &[negative.at(), 0, 10]), u64::MAX);
}

/// A number too long for the accumulator saturates instead of panicking.
///
/// Guest input is hostile by assumption. A debug build that overflowed here would abort the
/// emulator on a string, which is a worse outcome than any wrong number.
#[test]
fn an_absurdly_long_number_saturates_rather_than_panicking() {
    let digits = "9".repeat(200);
    let text = Text::new(&digits);
    let mut end: u64 = 0;
    let slot = std::ptr::from_mut(&mut end).expose_provenance() as u64;
    let _ = call("strtoull", &[text.at(), slot, 10]);
    assert_eq!(
        end,
        text.at() + 200,
        "every digit should still have been consumed"
    );
}

// --- absolute value --------------------------------------------------------------------

/// Absolute value, at each declared width.
#[test]
fn absolute_value_is_taken_at_the_declared_width() {
    assert_eq!(call("abs", &[u64::from((-5_i32) as u32)]) as i32, 5);
    assert_eq!(call("abs", &[5]) as i32, 5);
    assert_eq!(call("labs", &[(-5_i64) as u64]) as i64, 5);
    assert_eq!(call("llabs", &[(-5_i64) as u64]) as i64, 5);
}

/// The most negative value wraps rather than panicking.
///
/// Undefined in C, so there is no right answer to match - but there is a wrong outcome,
/// which is aborting the emulator in a debug build. Pinned so the choice stays deliberate.
#[test]
fn the_most_negative_value_wraps_instead_of_aborting() {
    assert_eq!(call("abs", &[u64::from(i32::MIN as u32)]) as i32, i32::MIN);
    assert_eq!(call("labs", &[i64::MIN as u64]) as i64, i64::MIN);
    assert_eq!(call("llabs", &[i64::MIN as u64]) as i64, i64::MIN);
}

// --- wide strings ----------------------------------------------------------------------

/// `wcslen` counts characters, not bytes.
///
/// The assumption under it is that `wchar_t` is 32-bit, as on every FreeBSD-derived system.
/// A 16-bit one would make this count double, and nothing in a trace would say so - which
/// is why the count, rather than a byte length, is what is asserted.
#[test]
fn wcslen_counts_characters_rather_than_bytes() {
    let text = Wide::new(&[u32::from(b'a'), u32::from(b'b'), 0x1F600]);
    assert_eq!(call("wcslen", &[text.at()]), 3);

    let empty = Wide::new(&[]);
    assert_eq!(call("wcslen", &[empty.at()]), 0);
}

/// A null wide string is length zero, not a fault.
#[test]
fn a_null_wide_string_is_length_zero() {
    assert_eq!(call("wcslen", &[0]), 0);
}

// --- null inputs -----------------------------------------------------------------------

/// Every string function tolerates a null pointer.
///
/// Not because passing one is legal C - it is not - but because a guest that does must not
/// take the emulator with it. The answer given is the empty-string answer in each case.
#[test]
fn a_null_string_is_treated_as_empty_rather_than_crashing() {
    assert_eq!(call("strstr", &[0, 0]), 0);
    assert_eq!(call("strpbrk", &[0, 0]), 0);
    assert_eq!(call("strspn", &[0, 0]), 0);
    assert_eq!(call("strcspn", &[0, 0]), 0);
    assert_eq!(call("strcasecmp", &[0, 0]), 0);
    assert_eq!(call("strncasecmp", &[0, 0, 8]), 0);
    assert_eq!(call("atoi", &[0]), 0);
    assert_eq!(call("strtol", &[0, 0, 10]), 0);
}

/// A null haystack with a real needle finds nothing, rather than treating the needle's
/// address as an offset into nowhere.
#[test]
fn a_null_haystack_with_a_real_needle_finds_nothing() {
    let needle = Text::new("x");
    assert_eq!(call("strstr", &[0, needle.at()]), 0);
    assert_eq!(call("strpbrk", &[0, needle.at()]), 0);
}
