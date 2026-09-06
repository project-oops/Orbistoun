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
fn parse_int(text: &[u8], base: u32) -> Parsed {
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
    // Where a `0x` began, so a prefix with nothing usable after it can fall back to the zero
    // it started with. See the refusal below for why that is not the same as reading nothing.
    let before_prefix = at;
    let mut took_prefix = false;
    if base == 0 || base == 16 {
        let prefix = text.get(at..at + 2);
        if matches!(prefix, Some([b'0', b'x' | b'X'])) {
            at += 2;
            base = 16;
            took_prefix = true;
        } else if base == 0 && text.get(at) == Some(&b'0') {
            base = 8;
        } else if base == 0 {
            base = 10;
        }
    }
    let start = at;
    // **The magnitude, unsigned and saturating** - not the signed value. A conversion that
    // overflows has to answer its type's limit, and which limit depends on the caller's type
    // and on the sign, neither of which this knows. Accumulating into a signed value and
    // truncating at the end answered a wrapped number instead: `strtoul` on twenty-three
    // nines returned `0x2c7e14af67fffff`, which is not merely wrong but *plausible*, so a
    // caller range-checking the result saw something in range and carried on (D479).
    let mut magnitude: u128 = 0;
    while let Some(digit) = text.get(at).and_then(|b| (*b as char).to_digit(base)) {
        magnitude = magnitude
            .saturating_mul(u128::from(base))
            .saturating_add(u128::from(digit));
        at += 1;
    }
    if at == start {
        // **`0x` with nothing usable after it is a conversion, not a refusal.** ISO C
        // 7.22.1.4 defines the subject sequence as the *longest initial subsequence of the
        // expected form*, and for `"0x"` that is `"0"` - so the value is zero and `endptr`
        // points at the `x`, one past the zero. Reading it as "no conversion" costs a caller
        // the difference between a parsed zero and a parse failure, which is the whole of
        // what `endptr` is for. Found by the differential against glibc (D498).
        if took_prefix {
            return Parsed {
                magnitude: 0,
                negative,
                consumed: before_prefix.saturating_add(1),
            };
        }
        // No digits at all: C says the value is zero and nothing was consumed, so an `endptr`
        // points back at the original string and a caller's loop terminates.
        return Parsed::default();
    }
    Parsed {
        magnitude,
        negative,
        consumed: at,
    }
}

/// What [`parse_int`] read, before any type's limits are applied to it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Parsed {
    /// The digits, as an unsigned magnitude. Saturates rather than wrapping.
    magnitude: u128,
    /// Whether a `-` preceded them.
    negative: bool,
    /// How many bytes were consumed, which is what an `endptr` reports.
    consumed: usize,
}

impl Parsed {
    /// The value an unsigned conversion of this width answers.
    ///
    /// **A negative subject is not an error and is not clamped to zero.** ISO C has an
    /// unsigned conversion negate the value, so `strtoul("-1")` is `ULONG_MAX` - which is the
    /// case that stops this being a plain clamp into range.
    const fn unsigned(self, highest: u64) -> u64 {
        if self.magnitude > highest as u128 {
            return highest;
        }
        let value = self.magnitude as u64;
        if self.negative {
            value.wrapping_neg()
        } else {
            value
        }
    }

    /// The value a signed conversion of this width answers.
    ///
    /// The two limits are not symmetric: the most negative value has a magnitude one greater
    /// than the most positive, and a conversion of exactly that magnitude is in range.
    const fn signed(self, lowest: i64, highest: i64) -> i64 {
        if self.negative {
            if self.magnitude > highest as u128 + 1 {
                return lowest;
            }
            return (self.magnitude as i64).wrapping_neg();
        }
        if self.magnitude > highest as u128 {
            return highest;
        }
        self.magnitude as i64
    }
}

/// Builds an `atoi`-family function of the given width.
macro_rules! ascii_to_int {
    ($name:ident, $width:ty) => {
        fn $name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
            // **Truncates where the `strtol` family clamps**, and the difference is the
            // standard's rather than an inconsistency: ISO C 7.22.1.2 leaves `atoi` on a
            // value it cannot represent *undefined*, while `strtol` is required to answer
            // its limit. A test pins the truncation, so it is a choice already made.
            let read = parse_int(&bytes(args[0]), 10);
            let value = read.magnitude as i128;
            (if read.negative { -value } else { value } as $width) as u64
        }
    };
}

ascii_to_int!(atoi, i32);
ascii_to_int!(atol, i64);
ascii_to_int!(atoll, i64);

/// Builds a `strtol`-family function, writing `endptr` when one was supplied.
macro_rules! string_to_int {
    ($name:ident, $answer:expr) => {
        fn $name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
            let text = bytes(args[0]);
            let base = u32::try_from(args[2]).unwrap_or(10);
            let read = parse_int(&text, base);
            let consumed = read.consumed;
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
            #[allow(
                clippy::redundant_closure_call,
                reason = "the limits differ per function, so each supplies its own"
            )]
            ($answer)(read)
        }
    };
}

// **Each supplies its own limits, because ISO C's are per type and per signedness.** A
// conversion that overflows answers the limit it overflowed, and answering the other one - or
// a wrapped value - is a number a caller cannot tell from a real answer.
string_to_int!(strtol, |r: Parsed| r.signed(i64::MIN, i64::MAX) as u64);
string_to_int!(strtoll, |r: Parsed| r.signed(i64::MIN, i64::MAX) as u64);
string_to_int!(strtoul, |r: Parsed| r.unsigned(u64::MAX));
string_to_int!(strtoull, |r: Parsed| r.unsigned(u64::MAX));

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

/// `wcsrchr(s, c)` - the address of the last wide character in `s` equal to `c`, or null.
///
/// Wide characters are four bytes, the 32-bit `wchar_t` [`wcslen`] assumes. The terminator is
/// part of the string, so `wcsrchr(s, 0)` answers a pointer to it, which the standard requires;
/// `c` is compared as a full 32-bit value, so its low half is what matters.
fn wcsrchr(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// As many wide characters as the byte limit allows, matching `wcslen`.
    const MAX_WIDE: u64 = 16 * 1024 * 1024;

    let (s, wanted) = (args[0], args[1] & 0xFFFF_FFFF);
    if s == 0 {
        return 0;
    }
    let mut last = 0_u64;
    let mut count = 0_u64;
    while count < MAX_WIDE {
        let address = s.saturating_add(count.saturating_mul(4));
        let Ok(at) = usize::try_from(address) else {
            break;
        };
        // SAFETY: a guest-supplied wide string under the identity mapping (D014), read one
        // character at a time so the scan cannot overrun a mapping by more than it reads.
        let wide = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u32>(at)) };
        if u64::from(wide) == wanted {
            last = address;
        }
        if wide == 0 {
            break;
        }
        count += 1;
    }
    last
}

/// Wide characters are four bytes here, as [`wcslen`] establishes.
const WIDE: usize = 4;

/// What an Annex K function answers when its runtime constraints are not met.
///
/// The standard requires only "a nonzero value" (ISO/IEC 9899:2011 K.3.6.1.1). A documented
/// errno is answered rather than an arbitrary number, because a guest printing it gets
/// something meaningful; the value is not load-bearing and nothing here depends on which it is.
const CONSTRAINT_VIOLATION: u64 = orbistoun_core::errno::INVALID as u64;

/// Reads at most `limit` wide characters of a guest string, stopping at its terminator.
fn wide_bytes(address: u64, limit: usize) -> Vec<u32> {
    let mut out = Vec::new();
    if address == 0 {
        return out;
    }
    for index in 0..limit {
        let Ok(base) = usize::try_from(address) else {
            break;
        };
        let at = base + index * WIDE;
        // SAFETY: a guest-supplied wide string under the identity mapping (D014), read one
        // character at a time so a scan cannot overrun a mapping by more than it reads.
        let value = unsafe { std::ptr::read_unaligned(at as *const u32) };
        if value == 0 {
            break;
        }
        out.push(value);
    }
    out
}

/// Copies `data` into guest memory at `at`.
fn write_bytes(at: u64, data: &[u8]) {
    if at == 0 || data.is_empty() {
        return;
    }
    // SAFETY: a guest-supplied buffer under the identity mapping (D014). Every caller has
    // already checked `data.len()` against the size the guest declared for the buffer, which
    // is the whole contract of the bounded functions below.
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr(at), data.len()) };
}

/// `strlcpy(dst, src, dstsize)` - a bounded copy that always terminates.
///
/// Reference: FreeBSD `strlcpy(3)`. **Answers the length of `src`, not how much was copied**,
/// so a caller can tell truncation from a fit by comparing it against `dstsize` - which is the
/// entire reason the function exists and the half that is easy to get wrong.
fn strlcpy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dst, src, size) = (args[0], args[1], args[2]);
    let source = bytes(src);
    let full = source.len() as u64;
    let Ok(size) = usize::try_from(size) else {
        return full;
    };
    if dst == 0 || size == 0 {
        return full;
    }
    // One byte of the space is the terminator, which is always written.
    let take = source.len().min(size - 1);
    write_bytes(dst, &source[..take]);
    write_bytes(dst + take as u64, &[0]);
    full
}

/// `strnstr(haystack, needle, len)` - `strstr` that will not read past `len` bytes.
///
/// Reference: FreeBSD `strnstr(3)`. An empty needle matches at the start, as `strstr` does.
fn strnstr(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (haystack, needle, len) = (args[0], args[1], args[2]);
    if haystack == 0 {
        return 0;
    }
    if needle == 0 {
        return haystack;
    }
    let wanted = bytes(needle);
    if wanted.is_empty() {
        return haystack;
    }
    let Ok(len) = usize::try_from(len) else {
        return 0;
    };
    let hay = bytes(haystack);
    // Bounded by the terminator *and* by `len`, whichever comes first.
    let bounded = &hay[..hay.len().min(len)];
    if bounded.len() < wanted.len() {
        return 0;
    }
    bounded
        .windows(wanted.len())
        .position(|window| window == wanted.as_slice())
        .map_or(0, |at| haystack + at as u64)
}

/// `wcscmp(a, b)` - the wide-character `strcmp`.
///
/// Reference: ISO/IEC 9899 7.29.4.4.1. Only the sign of the answer is defined.
fn wcscmp(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// As many wide characters as the byte limit allows, matching `wcslen`.
    const MAX_WIDE: usize = 4 * 1024 * 1024;
    let (a, b) = (wide_bytes(args[0], MAX_WIDE), wide_bytes(args[1], MAX_WIDE));
    let answer: i64 = match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    answer as u64
}

/// `wcsncpy(dst, src, n)` - at most `n` wide characters, padded with nulls.
///
/// Reference: ISO/IEC 9899 7.29.4.2.2. **Does not terminate when `src` is `n` or longer**, and
/// pads with nulls when it is shorter - both halves of a specification that surprises people.
fn wcsncpy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dst, src, n) = (args[0], args[1], args[2]);
    let Ok(n) = usize::try_from(n) else {
        return dst;
    };
    if dst == 0 {
        return dst;
    }
    let source = wide_bytes(src, n);
    let mut out = Vec::with_capacity(n * WIDE);
    for index in 0..n {
        let value = source.get(index).copied().unwrap_or(0);
        out.extend_from_slice(&value.to_le_bytes());
    }
    write_bytes(dst, &out);
    dst
}

/// The shared shape of an Annex K refusal: answer nonzero, and scrub what the caller gave.
///
/// Reference: ISO/IEC 9899:2011 K.3.7.1.1. On a constraint violation the destination is left
/// as an empty string rather than holding whatever it held, so a caller that ignores the return
/// value reads something safe instead of a stale buffer that looks like a result.
fn refuse_annex_k(s1: u64, s1max: u64, terminate_only: bool) -> u64 {
    if s1 != 0 && s1max > 0 {
        if terminate_only {
            write_bytes(s1, &[0]);
        } else if let Ok(size) = usize::try_from(s1max) {
            write_bytes(s1, &vec![0_u8; size]);
        }
    }
    CONSTRAINT_VIOLATION
}

/// `memcpy_s(s1, s1max, s2, n)` - a copy that checks the destination is big enough.
///
/// Reference: ISO/IEC 9899:2011 K.3.7.1.1. `RSIZE_MAX` is implementation-defined and is
/// deliberately not asserted: the constraint this exists for is `n <= s1max`.
fn memcpy_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s1, s1max, s2, n) = (args[0], args[1], args[2], args[3]);
    if s1 == 0 || s2 == 0 || n > s1max {
        return refuse_annex_k(s1, s1max, false);
    }
    let Ok(count) = usize::try_from(n) else {
        return refuse_annex_k(s1, s1max, false);
    };
    // SAFETY: guest buffers under the identity mapping (D014); `n <= s1max` was just checked,
    // so the destination has room for every byte read from the source.
    unsafe { std::ptr::copy_nonoverlapping(ptr(s2).cast_const(), ptr(s1), count) };
    0
}

/// `memmove_s(s1, s1max, s2, n)` - as [`memcpy_s`], and the regions may overlap.
///
/// Reference: ISO/IEC 9899:2011 K.3.7.1.2.
fn memmove_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s1, s1max, s2, n) = (args[0], args[1], args[2], args[3]);
    if s1 == 0 || s2 == 0 || n > s1max {
        return refuse_annex_k(s1, s1max, false);
    }
    let Ok(count) = usize::try_from(n) else {
        return refuse_annex_k(s1, s1max, false);
    };
    // SAFETY: guest buffers under the identity mapping (D014), `n <= s1max` checked. `copy`
    // rather than `copy_nonoverlapping` because overlapping is what this one permits.
    unsafe { std::ptr::copy(ptr(s2).cast_const(), ptr(s1), count) };
    0
}

/// `memset_s(s, smax, c, n)` - a fill that checks the buffer is big enough.
///
/// Reference: ISO/IEC 9899:2011 K.3.7.4.1. **The write happens even when it refuses**, which
/// is what makes it usable for scrubbing a secret: the standard requires the store not to be
/// optimised away or skipped on a constraint violation.
fn memset_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s, smax, c, n) = (args[0], args[1], args[2], args[3]);
    let Ok(size) = usize::try_from(smax) else {
        return CONSTRAINT_VIOLATION;
    };
    if s == 0 {
        return CONSTRAINT_VIOLATION;
    }
    let byte = (c & 0xFF) as u8;
    let violated = n > smax;
    let count = if violated {
        size
    } else {
        size.min(usize::try_from(n).unwrap_or(0))
    };
    write_bytes(s, &vec![byte; count]);
    if violated { CONSTRAINT_VIOLATION } else { 0 }
}

/// `strcat_s(s1, s1max, s2)` - an append that checks the result fits.
///
/// Reference: ISO/IEC 9899:2011 K.3.7.2.1.
fn strcat_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s1, s1max, s2) = (args[0], args[1], args[2]);
    if s1 == 0 || s2 == 0 {
        return refuse_annex_k(s1, s1max, true);
    }
    let (existing, addition) = (bytes(s1), bytes(s2));
    // The terminator has to fit as well as the characters.
    if existing.len() + addition.len() + 1 > usize::try_from(s1max).unwrap_or(0) {
        return refuse_annex_k(s1, s1max, true);
    }
    write_bytes(s1 + existing.len() as u64, &addition);
    write_bytes(s1 + (existing.len() + addition.len()) as u64, &[0]);
    0
}

/// `strncat_s(s1, s1max, s2, n)` - appends at most `n` characters, and always terminates.
///
/// Reference: ISO/IEC 9899:2011 K.3.7.2.2.
fn strncat_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s1, s1max, s2, n) = (args[0], args[1], args[2], args[3]);
    if s1 == 0 || s2 == 0 {
        return refuse_annex_k(s1, s1max, true);
    }
    let existing = bytes(s1);
    let mut addition = bytes(s2);
    addition.truncate(usize::try_from(n).unwrap_or(0));
    if existing.len() + addition.len() + 1 > usize::try_from(s1max).unwrap_or(0) {
        return refuse_annex_k(s1, s1max, true);
    }
    write_bytes(s1 + existing.len() as u64, &addition);
    write_bytes(s1 + (existing.len() + addition.len()) as u64, &[0]);
    0
}

/// `strncpy_s(s1, s1max, s2, n)` - a bounded copy that always terminates.
///
/// Reference: ISO/IEC 9899:2011 K.3.7.1.4. Unlike `strncpy` this **does** terminate, which is
/// the whole point of the `_s` variant.
fn strncpy_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s1, s1max, s2, n) = (args[0], args[1], args[2], args[3]);
    if s1 == 0 || s2 == 0 || s1max == 0 {
        return refuse_annex_k(s1, s1max, true);
    }
    let mut source = bytes(s2);
    source.truncate(usize::try_from(n).unwrap_or(0));
    if source.len() + 1 > usize::try_from(s1max).unwrap_or(0) {
        return refuse_annex_k(s1, s1max, true);
    }
    write_bytes(s1, &source);
    write_bytes(s1 + source.len() as u64, &[0]);
    0
}

/// `wcsncpy_s(s1, s1max, s2, n)` - [`strncpy_s`] in wide characters.
///
/// Reference: ISO/IEC 9899:2011 K.3.9.2.1.1. `s1max` and `n` count **wide characters**, not
/// bytes - the mistake that would make a bounds-checked function overrun by a factor of four.
fn wcsncpy_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (s1, s1max, s2, n) = (args[0], args[1], args[2], args[3]);
    let scrub = s1max.saturating_mul(WIDE as u64);
    if s1 == 0 || s2 == 0 || s1max == 0 {
        return refuse_annex_k(s1, scrub, true);
    }
    let source = wide_bytes(s2, usize::try_from(n).unwrap_or(0));
    if source.len() + 1 > usize::try_from(s1max).unwrap_or(0) {
        return refuse_annex_k(s1, scrub, true);
    }
    let mut out = Vec::with_capacity((source.len() + 1) * WIDE);
    for value in &source {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    write_bytes(s1, &out);
    0
}

/// `_Stoul(text, end, base)` - the runtime's own name for `strtoul`.
///
/// The Dinkumware runtime this platform carries (D468) puts the conversion in `_Stoul` and
/// makes `strtoul` a thin caller of it, so a guest built against it imports whichever of the
/// two its headers named. Delegated rather than reimplemented: two copies of a parser is two
/// answers to the same question, and the one nobody exercises is the one that drifts.
fn stoul(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    strtoul(args)
}

/// `_Stoull(text, end, base)` - as [`stoul`], at `unsigned long long`.
fn stoull(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    strtoull(args)
}

/// `strtoumax(text, end, base)` - ISO/IEC 9899 7.8.2.4.
///
/// `uintmax_t` is sixty-four bits on this target, which is what `strtoull` already converts to,
/// so this is that function under the name `<inttypes.h>` gives it. Delegated rather than
/// copied for the reason [`stoul`] is.
fn strtoumax(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    strtoull(args)
}

/// `strtoimax(text, end, base)` - ISO/IEC 9899 7.8.2.3.
///
/// The signed counterpart to `strtoumax`.
fn strtoimax(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    strtoll(args)
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
        ("_Stoul", stoul),
        ("_Stoull", stoull),
        ("strtoumax", strtoumax),
        ("strtoimax", strtoimax),
        ("abs", abs),
        ("labs", labs),
        ("llabs", llabs),
        ("wcslen", wcslen),
        ("strlcpy", strlcpy),
        ("strnstr", strnstr),
        ("wcscmp", wcscmp),
        ("wcsncpy", wcsncpy),
        ("memcpy_s", memcpy_s),
        ("memmove_s", memmove_s),
        ("memset_s", memset_s),
        ("strcat_s", strcat_s),
        ("strncat_s", strncat_s),
        ("strncpy_s", strncpy_s),
        ("wcsncpy_s", wcsncpy_s),
        ("wcsrchr", wcsrchr),
    ]
}

#[cfg(test)]
mod bounded_strings {
    use super::*;

    /// A guest buffer this process owns, so the identity mapping the functions assume holds.
    fn buffer(contents: &[u8], spare: usize) -> (Box<[u8]>, u64) {
        let mut owned = contents.to_vec();
        owned.resize(contents.len() + spare, 0);
        let mut boxed = owned.into_boxed_slice();
        let at = boxed.as_mut_ptr() as u64;
        (boxed, at)
    }

    fn call(f: GuestFn, args: [u64; 4]) -> u64 {
        let mut full = [0_u64; GUEST_ARG_REGISTERS];
        full[..4].copy_from_slice(&args);
        f(&full)
    }

    fn text(at: &[u8]) -> &[u8] {
        let end = at.iter().position(|b| *b == 0).unwrap_or(at.len());
        &at[..end]
    }

    /// `strlcpy` answers the length of the **source**, which is what lets a caller detect
    /// truncation. Answering the copied length would look right in every fitting case.
    #[test]
    fn strlcpy_answers_the_source_length_even_when_it_truncates() {
        let (src, src_at) = buffer(b"abcdef\0", 0);
        let (mut dst, dst_at) = buffer(b"", 4);
        let answer = call(strlcpy, [dst_at, src_at, 4, 0]);
        assert_eq!(answer, 6, "the answer is the source length, not the copy");
        assert_eq!(text(&dst), b"abc", "three characters and a terminator");
        dst[0] = 0;
        drop(src);
    }

    /// A zero-sized destination is written to at all, and the source length still answered.
    #[test]
    fn strlcpy_writes_nothing_into_a_zero_sized_destination() {
        let (src, src_at) = buffer(b"abc\0", 0);
        let (dst, dst_at) = buffer(b"XY\0", 0);
        let answer = call(strlcpy, [dst_at, src_at, 0, 0]);
        assert_eq!(answer, 3);
        assert_eq!(text(&dst), b"XY", "untouched, because there was no room");
        drop(src);
    }

    /// `strnstr` must not look past its limit, which is the only thing separating it from
    /// `strstr` - a match starting inside the window but running past it does not count.
    #[test]
    fn strnstr_will_not_match_past_its_limit() {
        let (hay, hay_at) = buffer(b"abcdef\0", 0);
        let (needle, needle_at) = buffer(b"de\0", 0);
        assert_eq!(
            call(strnstr, [hay_at, needle_at, 6, 0]),
            hay_at + 3,
            "inside the window, so found"
        );
        assert_eq!(
            call(strnstr, [hay_at, needle_at, 4, 0]),
            0,
            "the match would run past the limit, so it is not a match"
        );
        drop((hay, needle));
    }

    /// **The Annex K guard, made to fail.** A copy larger than the destination must be refused
    /// *and* the destination scrubbed - a caller ignoring the return value must not find a
    /// half-written buffer that reads like a result.
    #[test]
    fn memcpy_s_refuses_and_scrubs_when_the_destination_is_too_small() {
        let (src, src_at) = buffer(b"abcdefgh", 0);
        let (dst, dst_at) = buffer(b"ZZZZ", 0);
        let answer = call(memcpy_s, [dst_at, 4, src_at, 8]);
        assert_ne!(answer, 0, "a constraint violation answers nonzero");
        assert_eq!(&dst[..4], b"\0\0\0\0", "and the destination is scrubbed");
        drop(src);
    }

    /// The fitting case, so the guard above is known to be discriminating.
    #[test]
    fn memcpy_s_copies_when_it_fits() {
        let (src, src_at) = buffer(b"abcd", 0);
        let (dst, dst_at) = buffer(b"ZZZZ", 0);
        assert_eq!(call(memcpy_s, [dst_at, 4, src_at, 4]), 0);
        assert_eq!(&dst[..4], b"abcd");
        drop(src);
    }

    /// `memset_s` writes **even when it refuses**, which is what makes it usable to scrub a
    /// secret. A version that returned early on the violation would leave the secret there.
    #[test]
    fn memset_s_still_writes_when_it_refuses() {
        let (dst, dst_at) = buffer(b"secret", 0);
        let answer = call(memset_s, [dst_at, 6, 0, 99]);
        assert_ne!(answer, 0, "n greater than smax is a violation");
        assert_eq!(
            &dst[..6],
            b"\0\0\0\0\0\0",
            "and the buffer is cleared anyway"
        );
    }

    /// `strncpy_s` terminates where `strncpy` would not - the reason the variant exists.
    #[test]
    fn strncpy_s_always_terminates() {
        let (src, src_at) = buffer(b"abc\0", 0);
        let (dst, dst_at) = buffer(b"ZZZZZZ", 0);
        assert_eq!(call(strncpy_s, [dst_at, 6, src_at, 3]), 0);
        assert_eq!(text(&dst), b"abc");
        assert_eq!(dst[3], 0, "terminated, unlike strncpy");
        drop(src);
    }

    /// An append whose result would not fit, terminator included, is refused and the
    /// destination emptied.
    #[test]
    fn strcat_s_refuses_an_append_that_would_not_fit() {
        let (src, src_at) = buffer(b"cde\0", 0);
        let (dst, dst_at) = buffer(b"ab\0\0\0", 0);
        let answer = call(strcat_s, [dst_at, 5, src_at, 0]);
        assert_ne!(answer, 0, "2 + 3 + terminator needs 6, and there are 5");
        assert_eq!(text(&dst), b"", "and the destination is left empty");
        drop(src);
    }

    /// `wcsncpy_s` counts **wide characters**, not bytes. Counting bytes would let four times
    /// too much through a bounds check, which is the one mistake that matters here.
    #[test]
    fn wcsncpy_s_counts_wide_characters_not_bytes() {
        let src: Box<[u32]> = vec![u32::from(b'a'), u32::from(b'b'), 0].into_boxed_slice();
        let src_at = src.as_ptr() as u64;
        let dst: Box<[u32]> = vec![0_u32; 8].into_boxed_slice();
        let dst_at = dst.as_ptr() as u64;
        // Two characters plus a terminator need three; a limit of two must refuse.
        assert_ne!(
            call(wcsncpy_s, [dst_at, 2, src_at, 2]),
            0,
            "two wide characters and a terminator do not fit in two"
        );
        assert_eq!(call(wcsncpy_s, [dst_at, 3, src_at, 2]), 0);
        // SAFETY: the buffer above, which this test owns and sized at eight characters.
        let written = unsafe { std::slice::from_raw_parts(dst_at as *const u32, 3) };
        assert_eq!(written, [u32::from(b'a'), u32::from(b'b'), 0]);
        drop(src);
    }

    /// `wcsncpy` pads with nulls and does *not* terminate a source that fills the field -
    /// both halves of a specification that surprises people.
    #[test]
    fn wcsncpy_pads_but_does_not_terminate_a_full_field() {
        let src: Box<[u32]> = vec![u32::from(b'a'), u32::from(b'b'), 0].into_boxed_slice();
        let src_at = src.as_ptr() as u64;
        let dst: Box<[u32]> = vec![0xFFFF_FFFF_u32; 4].into_boxed_slice();
        let dst_at = dst.as_ptr() as u64;
        call(wcsncpy, [dst_at, src_at, 4, 0]);
        // SAFETY: this test's own buffer, four characters wide.
        let written = unsafe { std::slice::from_raw_parts(dst_at as *const u32, 4) };
        assert_eq!(
            written,
            [u32::from(b'a'), u32::from(b'b'), 0, 0],
            "padded with nulls"
        );

        let exact: Box<[u32]> = vec![0xFFFF_FFFF_u32; 2].into_boxed_slice();
        let exact_at = exact.as_ptr() as u64;
        call(wcsncpy, [exact_at, src_at, 2, 0]);
        // SAFETY: as above, two characters wide.
        let filled = unsafe { std::slice::from_raw_parts(exact_at as *const u32, 2) };
        assert_eq!(
            filled,
            [u32::from(b'a'), u32::from(b'b')],
            "no room left to terminate"
        );
        drop(src);
    }

    /// Only the sign of `wcscmp` is defined, so that is all this asserts.
    #[test]
    fn wcscmp_orders_by_the_first_difference() {
        let a: Box<[u32]> = vec![u32::from(b'a'), u32::from(b'b'), 0].into_boxed_slice();
        let b: Box<[u32]> = vec![u32::from(b'a'), u32::from(b'c'), 0].into_boxed_slice();
        let (a_at, b_at) = (a.as_ptr() as u64, b.as_ptr() as u64);
        assert_eq!(call(wcscmp, [a_at, a_at, 0, 0]), 0);
        assert!((call(wcscmp, [a_at, b_at, 0, 0]) as i64) < 0);
        assert!((call(wcscmp, [b_at, a_at, 0, 0]) as i64) > 0);
    }
}
