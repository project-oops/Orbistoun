//! The string, character-class and integer-conversion parts of the C library.
//!
//! # Why these arrive as one batch
//!
//! The conformance probe's `035-libc` section failed sixteen checks, and almost every one
//! named a function that simply was not there - `strstr`, `strspn`, `toupper`, `atol`,
//! `strtoul`, `strncasecmp`. They are defined by the C standard rather than by the
//! platform, so there is one right answer per function and no room for a guess (D270).
//!
//! # The bytes are the guest's, and none of this assumes they are text
//!
//! A C string is bytes terminated by NUL, not UTF-8. Everything here works on `u8` and
//! uses the C locale's classification rules, which are defined only for ASCII - a byte
//! above 127 is not a letter here, and treating it as one is how a locale-dependent
//! answer becomes a wrong one on somebody else's machine.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

use crate::{c_len, ptr};

/// Reads a guest string as bytes, bounded by its own terminator.
fn bytes(address: u64) -> Vec<u8> {
    if address == 0 {
        return Vec::new();
    }
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded by the
    // same limit every other string function here uses.
    let len = unsafe { c_len(address) };
    // SAFETY: `len` bytes are readable by the scan that just measured them.
    unsafe { std::slice::from_raw_parts(ptr(address), len) }.to_vec()
}

/// C's idea of a lower-case letter: ASCII only, because the C locale defines nothing else.
const fn is_lower(b: u8) -> bool {
    b.is_ascii_lowercase()
}

/// C's idea of an upper-case letter.
const fn is_upper(b: u8) -> bool {
    b.is_ascii_uppercase()
}

/// Folds one byte to lower case, ASCII only.
const fn fold(b: u8) -> u8 {
    b.to_ascii_lowercase()
}

// --- character classes -------------------------------------------------------------
//
// Each takes an `int` and returns non-zero or zero. **The argument is an `int`, not a
// `char`**: C requires it to be representable as `unsigned char` or equal to `EOF`, and a
// guest passing a sign-extended byte would otherwise index out of a table. Here the value
// is simply masked, which answers `false` for anything outside a byte - including `EOF`,
// which is not a member of any class.

/// Builds a classification function from a predicate on the byte.
macro_rules! class {
    ($name:ident, $test:expr) => {
        fn $name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
            let Ok(b) = u8::try_from(args[0] & 0xFF) else {
                return 0;
            };
            // Anything that did not fit in a byte is not in any class.
            if args[0] > 0xFF && args[0] as i64 >= 0 {
                return 0;
            }
            u64::from($test(b))
        }
    };
}

class!(isalpha, |b: u8| b.is_ascii_alphabetic());
class!(isdigit, |b: u8| b.is_ascii_digit());
class!(isalnum, |b: u8| b.is_ascii_alphanumeric());
class!(isspace, |b: u8| b.is_ascii_whitespace() || b == 0x0b);
class!(isupper, is_upper);
class!(islower, is_lower);
class!(ispunct, |b: u8| b.is_ascii_punctuation());
class!(isxdigit, |b: u8| b.is_ascii_hexdigit());
class!(iscntrl, |b: u8| b.is_ascii_control());
class!(isprint, |b: u8| (0x20..0x7f).contains(&b));
class!(isgraph, |b: u8| b.is_ascii_graphic());

/// `toupper(c)` - unchanged when it is not a lower-case letter.
fn toupper(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let b = (args[0] & 0xFF) as u8;
    if is_lower(b) {
        u64::from(b - 32)
    } else {
        args[0]
    }
}

/// `tolower(c)`.
fn tolower(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let b = (args[0] & 0xFF) as u8;
    if is_upper(b) {
        u64::from(b + 32)
    } else {
        args[0]
    }
}

// --- searching ---------------------------------------------------------------------

/// `strstr(haystack, needle)` - the address of the first match, or null.
///
/// An empty needle matches at the start, which the standard requires and which a naive
/// search returns null for.
fn strstr(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (haystack, needle) = (bytes(args[0]), bytes(args[1]));
    if needle.is_empty() {
        return args[0];
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle.as_slice())
        .map_or(0, |at| args[0].saturating_add(at as u64))
}

/// `strpbrk(text, accept)` - the first byte of `text` that appears in `accept`.
fn strpbrk(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (text, accept) = (bytes(args[0]), bytes(args[1]));
    text.iter()
        .position(|b| accept.contains(b))
        .map_or(0, |at| args[0].saturating_add(at as u64))
}

/// `strspn(text, accept)` - how many leading bytes are all in `accept`.
fn strspn(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (text, accept) = (bytes(args[0]), bytes(args[1]));
    text.iter().take_while(|b| accept.contains(b)).count() as u64
}

/// `strcspn(text, reject)` - how many leading bytes are in none of `reject`.
fn strcspn(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (text, reject) = (bytes(args[0]), bytes(args[1]));
    text.iter().take_while(|b| !reject.contains(b)).count() as u64
}

/// `strcasecmp(a, b)` - comparison ignoring ASCII case.
fn strcasecmp(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (a, b) = (bytes(args[0]), bytes(args[1]));
    order(a.iter().map(|c| fold(*c)), b.iter().map(|c| fold(*c)))
}

/// `strncasecmp(a, b, n)` - the same, over at most `n` bytes.
fn strncasecmp(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let n = usize::try_from(args[2]).unwrap_or(usize::MAX);
    let (a, b) = (bytes(args[0]), bytes(args[1]));
    order(
        a.iter().take(n).map(|c| fold(*c)),
        b.iter().take(n).map(|c| fold(*c)),
    )
}

/// The sign of a byte-wise comparison, as C reports it.
fn order(a: impl Iterator<Item = u8>, b: impl Iterator<Item = u8>) -> u64 {
    let (a, b): (Vec<u8>, Vec<u8>) = (a.collect(), b.collect());
    match a.cmp(&b) {
        std::cmp::Ordering::Less => (-1_i64) as u64,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

// --- integer conversion ------------------------------------------------------------

/// Parses a C integer: optional space, optional sign, digits in `base`.
///
/// `base` of zero means "work it out from the prefix", which is what `strtol` does and
/// what `atoi` must not do - `atoi("010")` is ten, not eight.
///
/// Returns the value and how many bytes were consumed, so `strtol` can report where it
/// stopped. **A caller uses that to walk a list**, and one that never advances is a hang
/// rather than a wrong answer.
fn parse_int(text: &[u8], base: u32) -> (i128, usize) {
    let mut at = 0;
    while at < text.len() && (text[at].is_ascii_whitespace() || text[at] == 0x0b) {
        at += 1;
    }
    let negative = match text.get(at) {
        Some(b'-') => {
            at += 1;
            true
        }
        Some(b'+') => {
            at += 1;
            false
        }
        _ => false,
    };
    let mut base = base;
    if base == 0 || base == 16 {
        let prefix = text.get(at..at + 2);
        if matches!(prefix, Some([b'0', b'x' | b'X'])) {
            at += 2;
            base = 16;
        } else if base == 0 && text.get(at) == Some(&b'0') {
            base = 8;
        } else if base == 0 {
            base = 10;
        }
    }
    let start = at;
    let mut value: i128 = 0;
    while let Some(digit) = text.get(at).and_then(|b| (*b as char).to_digit(base)) {
        value = value
            .saturating_mul(i128::from(base))
            .saturating_add(i128::from(digit));
        at += 1;
    }
    // No digits at all: C says the value is zero and nothing was consumed, so an `endptr`
    // points back at the original string and a caller's loop terminates.
    if at == start {
        return (0, 0);
    }
    (if negative { -value } else { value }, at)
}

/// Builds an `atoi`-family function of the given width.
macro_rules! ascii_to_int {
    ($name:ident, $width:ty) => {
        fn $name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
            let (value, _) = parse_int(&bytes(args[0]), 10);
            (value as $width) as u64
        }
    };
}

ascii_to_int!(atoi, i32);
ascii_to_int!(atol, i64);
ascii_to_int!(atoll, i64);

/// Builds a `strtol`-family function, writing `endptr` when one was supplied.
macro_rules! string_to_int {
    ($name:ident, $width:ty) => {
        fn $name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
            let text = bytes(args[0]);
            let base = u32::try_from(args[2]).unwrap_or(10);
            let (value, consumed) = parse_int(&text, base);
            if args[1] != 0 {
                if let Ok(at) = usize::try_from(args[1]) {
                    // SAFETY: a guest-supplied `char **` under the identity mapping (D014),
                    // written only when the guest passed a non-null pointer.
                    unsafe {
                        std::ptr::write_unaligned(
                            std::ptr::with_exposed_provenance_mut::<u64>(at),
                            args[0].saturating_add(consumed as u64),
                        );
                    }
                }
            }
            (value as $width) as u64
        }
    };
}

string_to_int!(strtol, i64);
string_to_int!(strtoll, i64);
string_to_int!(strtoul, u64);
string_to_int!(strtoull, u64);

/// Builds an absolute-value function.
///
/// **`abs(INT_MIN)` is undefined in C and must not panic here.** Wrapping is what the
/// hardware does and what every real implementation returns, so that is what happens -
/// stated rather than left to a debug build to discover.
macro_rules! absolute {
    ($name:ident, $width:ty) => {
        fn $name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
            let value = args[0] as $width;
            (value.wrapping_abs() as $width) as u64
        }
    };
}

absolute!(abs, i32);
absolute!(labs, i64);
absolute!(llabs, i64);

/// `wcslen(text)` - wide characters, which are four bytes here.
///
/// The target's `wchar_t` is 32-bit, as it is on every FreeBSD-derived system. Recorded as
/// an assumption: a 16-bit `wchar_t` would make this count double and nothing in a trace
/// would say so.
fn wcslen(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// As many wide characters as the byte limit allows.
    const MAX_WIDE: u64 = 16 * 1024 * 1024;

    if args[0] == 0 {
        return 0;
    }
    let mut count = 0;
    while count < MAX_WIDE {
        let Ok(at) = usize::try_from(args[0].saturating_add(count.saturating_mul(4))) else {
            break;
        };
        // SAFETY: a guest-supplied wide string under the identity mapping (D014), read one
        // character at a time so the scan cannot overrun a mapping by more than it reads.
        let wide = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u32>(at)) };
        if wide == 0 {
            break;
        }
        count += 1;
    }
    count
}

/// Everything here, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("isalpha", isalpha),
        ("isdigit", isdigit),
        ("isalnum", isalnum),
        ("isspace", isspace),
        ("isupper", isupper),
        ("islower", islower),
        ("ispunct", ispunct),
        ("isxdigit", isxdigit),
        ("iscntrl", iscntrl),
        ("isprint", isprint),
        ("isgraph", isgraph),
        ("toupper", toupper),
        ("tolower", tolower),
        ("strstr", strstr),
        ("strpbrk", strpbrk),
        ("strspn", strspn),
        ("strcspn", strcspn),
        ("strcasecmp", strcasecmp),
        ("strncasecmp", strncasecmp),
        ("atoi", atoi),
        ("atol", atol),
        ("atoll", atoll),
        ("strtol", strtol),
        ("strtoll", strtoll),
        ("strtoul", strtoul),
        ("strtoull", strtoull),
        ("abs", abs),
        ("labs", labs),
        ("llabs", llabs),
        ("wcslen", wcslen),
    ]
}
