//! Reading a formatted string, and writing a formatted time.
//!
//! # The two calls between `ftpsrv` and a listening port
//!
//! Measured: with everything else implemented, `ftpsrv` imports exactly two names nothing here
//! serves - `sscanf` and `strftime`. A file server parses the numbers out of a command with
//! one and stamps a directory listing with the other, and it does both before it says anything
//! useful.
//!
//! # `sscanf` is `printf` run backwards, and has the same limit
//!
//! Arguments arrive in registers, six of them, two spent on the string and the format. So four
//! conversions can be assigned and a fifth cannot - the pointer for it was passed on the stack,
//! which the trampoline does not reach (the same wall `snprintf` documents).
//!
//! **A format that needs more is refused entirely**, rather than assigning the four it can.
//! Returning a partial count is worse than returning none: `sscanf`'s contract is that the
//! count says how many conversions succeeded, so a caller that asked for six and is told four
//! believes the first four are good - and here they might be, or the arguments might have been
//! misaligned from the start. Refusing is the same choice the renderer makes and for the same
//! reason (principle 3).
//!
//! # `struct tm` is citable, and its layout is the whole of `strftime`
//!
//! ```text
//! include/time.h
//!     int  tm_sec, tm_min, tm_hour, tm_mday, tm_mon, tm_year, tm_wday, tm_yday, tm_isdst;
//!     long tm_gmtoff;
//!     char *tm_zone;
//! ```
//!
//! Nine `int`s then a `long` - so the integers are at offsets 0 to 32 in four-byte steps, and
//! nothing this needs lies past them.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

use crate::{c_len, ptr};

/// Conversions this can assign, being the argument registers less the two fixed ones.
const ASSIGNABLE: usize = GUEST_ARG_REGISTERS - 2;

/// What `sscanf` answers when the input ended before anything matched.
///
/// `EOF`, which is negative one widened - and distinct from zero, which means the input was
/// there and did not match.
const NO_INPUT: u64 = -1_i64 as u64;

/// Reads a NUL-terminated guest string as bytes.
///
/// # Safety
///
/// `address` must name a NUL-terminated string in guest memory, which is the contract every
/// string function here has under the identity mapping (D014).
unsafe fn guest_str<'a>(address: u64) -> Option<&'a [u8]> {
    if address == 0 {
        return None;
    }
    // SAFETY: the caller's contract.
    let len = unsafe { c_len(address) };
    // SAFETY: `c_len` established `len` readable bytes.
    Some(unsafe { std::slice::from_raw_parts(ptr(address).cast_const(), len) })
}

/// One thing a format asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wanted {
    /// A signed integer, in the given base.
    Integer(u32),
    /// An unsigned integer, in the given base.
    Unsigned(u32),
    /// A run of non-space characters.
    String,
    /// Exactly one character.
    Character,
}

impl Wanted {
    /// How many bytes the destination holds, for the numeric kinds.
    const fn width(self, long: bool) -> usize {
        match self {
            Self::Integer(_) | Self::Unsigned(_) if long => 8,
            Self::Integer(_) | Self::Unsigned(_) => 4,
            Self::String | Self::Character => 1,
        }
    }
}

/// `sscanf(input, format, ...)`.
///
/// Supports the conversions a command parser uses: `%d`, `%i`, `%u`, `%x`, `%o`, `%s`, `%c`,
/// a literal `%%`, a maximum field width, and the assignment-suppressing `*`. Whitespace in
/// the format matches any run of whitespace, as the standard says, and any other character
/// must match itself.
///
/// Answers how many conversions were assigned, or `EOF` when the input ran out before the
/// first one - which is the distinction a caller loops on.
///
/// Reference: ISO C `sscanf`; POSIX.1-2008 `sscanf(3)`.
fn sscanf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: guest-supplied strings under the identity mapping (D014).
    let (Some(input), Some(format)) =
        (unsafe { guest_str(args[0]) }, unsafe { guest_str(args[1]) })
    else {
        return NO_INPUT;
    };

    let mut destinations = args[2..].iter().copied();
    let mut at = 0_usize;
    let mut assigned = 0_u64;
    let mut format = format.iter().copied().peekable();

    while let Some(byte) = format.next() {
        if byte.is_ascii_whitespace() {
            while input.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            continue;
        }
        if byte != b'%' {
            if input.get(at) != Some(&byte) {
                return assigned;
            }
            at += 1;
            continue;
        }

        let suppress = format.peek() == Some(&b'*');
        if suppress {
            format.next();
        }
        let mut width = 0_usize;
        while let Some(&digit) = format.peek() {
            if !digit.is_ascii_digit() {
                break;
            }
            width = width
                .saturating_mul(10)
                .saturating_add(usize::from(digit - b'0'));
            format.next();
        }
        let mut long = false;
        while matches!(format.peek(), Some(b'l' | b'h' | b'z' | b'j' | b't' | b'L')) {
            long |= format.peek() == Some(&b'l');
            format.next();
        }

        let Some(conversion) = format.next() else {
            return assigned;
        };
        if conversion == b'%' {
            if input.get(at) != Some(&b'%') {
                return assigned;
            }
            at += 1;
            continue;
        }

        let wanted = match conversion {
            b'd' | b'i' => Wanted::Integer(10),
            b'u' => Wanted::Unsigned(10),
            b'x' | b'X' => Wanted::Unsigned(16),
            b'o' => Wanted::Unsigned(8),
            b's' => Wanted::String,
            b'c' => Wanted::Character,
            // A conversion this cannot read stops the scan rather than skipping it: carrying
            // on would assign the *next* argument from the wrong place.
            _ => return assigned,
        };

        // **The register wall.** A destination past the sixth argument was passed on the
        // stack, which the trampoline does not reach - so the whole call is refused rather
        // than partly performed.
        let destination = if suppress {
            0
        } else {
            let Some(destination) = destinations.next() else {
                return 0;
            };
            if assigned as usize >= ASSIGNABLE {
                return 0;
            }
            destination
        };

        let taken = match wanted {
            Wanted::Character => scan_character(input, at, destination, suppress),
            Wanted::String => scan_string(input, at, width, destination, suppress),
            Wanted::Integer(base) | Wanted::Unsigned(base) => scan_number(
                input,
                at,
                base,
                width,
                destination,
                suppress,
                wanted.width(long),
            ),
        };
        let Some(taken) = taken else {
            return assigned;
        };
        at += taken;
        if !suppress {
            assigned += 1;
        }
    }
    assigned
}

/// Reads one character, answering how much input it took.
fn scan_character(input: &[u8], at: usize, destination: u64, suppress: bool) -> Option<usize> {
    let byte = *input.get(at)?;
    if !suppress {
        write_bytes(destination, &[byte]);
    }
    Some(1)
}

/// Reads a run of non-space characters, terminated, answering how much input it took.
fn scan_string(
    input: &[u8],
    at: usize,
    width: usize,
    destination: u64,
    suppress: bool,
) -> Option<usize> {
    let mut end = at;
    while end < input.len() && !input[end].is_ascii_whitespace() {
        if width > 0 && end - at >= width {
            break;
        }
        end += 1;
    }
    if end == at {
        return None;
    }
    if !suppress {
        let mut bytes = input[at..end].to_vec();
        bytes.push(0);
        write_bytes(destination, &bytes);
    }
    Some(end - at)
}

/// Reads a number, answering how much input it took.
fn scan_number(
    input: &[u8],
    at: usize,
    base: u32,
    width: usize,
    destination: u64,
    suppress: bool,
    bytes: usize,
) -> Option<usize> {
    let mut cursor = at;
    // Leading whitespace is skipped by every numeric conversion, which is the standard's rule
    // and not the same as the format containing a space.
    while input.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    let start = cursor;
    if matches!(input.get(cursor), Some(b'-' | b'+')) {
        cursor += 1;
    }
    while cursor < input.len() && (input[cursor] as char).is_digit(base) {
        if width > 0 && cursor - start >= width {
            break;
        }
        cursor += 1;
    }
    if cursor == start || (cursor == start + 1 && !input[start].is_ascii_digit()) {
        return None;
    }
    let text = std::str::from_utf8(&input[start..cursor]).ok()?;
    let value = i64::from_str_radix(text, base).ok()?;
    if !suppress {
        if bytes == 8 {
            write_bytes(destination, &value.to_le_bytes());
        } else {
            write_bytes(destination, &(value as i32).to_le_bytes());
        }
    }
    Some(cursor - at)
}

/// Writes bytes where a guest asked for them.
fn write_bytes(destination: u64, bytes: &[u8]) {
    if destination == 0 {
        return;
    }
    let Ok(at) = usize::try_from(destination) else {
        return;
    };
    // SAFETY: a guest-supplied destination under the identity mapping (D014), which the guest
    // promised is large enough - the same promise the real call relies on.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            bytes.len(),
        );
    }
}

/// Reads a `struct tm` a guest passed.
///
/// Only the nine leading `int`s, which is everything the conversions below need: the offset
/// and the zone name sit past them and nothing here reports a zone.
fn read_tm(address: u64) -> Option<[i32; 9]> {
    let at = usize::try_from(address).ok()?;
    if at == 0 {
        return None;
    }
    let base = std::ptr::with_exposed_provenance::<i32>(at);
    let mut out = [0_i32; 9];
    for (index, field) in out.iter_mut().enumerate() {
        // SAFETY: a guest-supplied `struct tm` under the identity mapping (D014), whose first
        // nine fields are `int` per `include/time.h`.
        let slot = unsafe { base.add(index) };
        // SAFETY: in bounds by the line above.
        *field = unsafe { std::ptr::read_unaligned(slot) };
    }
    Some(out)
}

/// Day names, as `%a` and `%A` render them.
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Month names, as `%b` and `%B` render them.
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `strftime(dest, max, format, tm)`.
///
/// Supports what a listing or a log line uses: `%Y %m %d %H %M %S %y %j %b %B %a %A %p %e %F
/// %T %D %R %n %t %%`. A conversion it cannot render **stops the whole thing** and answers
/// zero, as the interface says it must when the result does not fit - a half-rendered
/// timestamp is a wrong date rather than a short one.
///
/// **No timezone and no locale.** `%Z` and `%z` would need the fields past the nine this reads
/// and a zone this emulator has no notion of, so they are among the conversions that stop it.
/// The names are the C locale's, which is what a file listing wants.
///
/// Reference: ISO C `strftime`; POSIX.1-2008 `strftime(3)`; `struct tm` from `include/time.h`.
fn strftime(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, max, format, tm) = (args[0], args[1] as usize, args[2], args[3]);
    // SAFETY: a guest-supplied string under the identity mapping (D014).
    let (Some(format), Some(tm)) = (unsafe { guest_str(format) }, read_tm(tm)) else {
        return 0;
    };
    if dest == 0 || max == 0 {
        return 0;
    }

    let [sec, min, hour, mday, mon, year, wday, yday, _isdst] = tm;
    let mut out: Vec<u8> = Vec::with_capacity(format.len() * 2);
    let mut chars = format.iter().copied().peekable();
    while let Some(byte) = chars.next() {
        if byte != b'%' {
            out.push(byte);
            continue;
        }
        let Some(conversion) = chars.next() else {
            return 0;
        };
        let rendered = match conversion {
            b'%' => "%".to_owned(),
            b'n' => "\n".to_owned(),
            b't' => "\t".to_owned(),
            b'Y' => format!("{}", year + 1900),
            b'y' => format!("{:02}", (year + 1900).rem_euclid(100)),
            b'm' => format!("{:02}", mon + 1),
            b'd' => format!("{mday:02}"),
            // Space-padded rather than zero-padded, which is the difference between `%e` and
            // `%d` and the reason a listing uses it.
            b'e' => format!("{mday:2}"),
            b'H' => format!("{hour:02}"),
            b'M' => format!("{min:02}"),
            b'S' => format!("{sec:02}"),
            b'j' => format!("{:03}", yday + 1),
            b'p' => (if hour < 12 { "AM" } else { "PM" }).to_owned(),
            // A `tm_wday` of nine is a guest with a broken structure, and rendering
            // "Wednesday" for it would hide that - so an index the table does not hold stops
            // the whole thing, exactly as an unrenderable conversion does.
            b'a' => match name(&DAYS, wday).and_then(|day| day.get(..3)) {
                Some(short) => short.to_owned(),
                None => return 0,
            },
            b'A' => match name(&DAYS, wday) {
                Some(day) => day.to_owned(),
                None => return 0,
            },
            b'b' | b'h' => match name(&MONTHS, mon).and_then(|month| month.get(..3)) {
                Some(short) => short.to_owned(),
                None => return 0,
            },
            b'B' => match name(&MONTHS, mon) {
                Some(month) => month.to_owned(),
                None => return 0,
            },
            b'F' => format!("{}-{:02}-{:02}", year + 1900, mon + 1, mday),
            b'T' => format!("{hour:02}:{min:02}:{sec:02}"),
            b'R' => format!("{hour:02}:{min:02}"),
            b'D' => format!(
                "{:02}/{:02}/{:02}",
                mon + 1,
                mday,
                (year + 1900).rem_euclid(100)
            ),
            // Anything else - a zone, a locale form, a modifier - stops it. A half-rendered
            // timestamp is a wrong date rather than a short one.
            _ => return 0,
        };
        out.extend_from_slice(rendered.as_bytes());
    }

    // One byte for the terminator, which is what makes the count meaningful.
    if out.len() + 1 > max {
        return 0;
    }
    out.push(0);
    write_bytes(dest, &out);
    (out.len() - 1) as u64
}

/// A name from a table, by an index a guest supplied.
///
/// [`None`] rather than a wrap, because a `tm_wday` of nine is a guest with a broken structure
/// and rendering "Wednesday" for it would hide that.
fn name(table: &[&'static str], index: i32) -> Option<&'static str> {
    table.get(usize::try_from(index).ok()?).copied()
}

/// Implementations this module provides, by symbol name.
pub(crate) fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sscanf", sscanf), ("strftime", strftime)]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .expect("declared");
        function(&args)
    }

    #[test]
    fn numbers_and_words_come_out_where_they_were_asked_for() {
        let mut port = 0_i32;
        let mut name = [0_u8; 16];
        let matched = call(
            "sscanf",
            [
                c"PORT 2121 data".as_ptr() as u64,
                c"%s %d".as_ptr() as u64,
                name.as_mut_ptr() as u64,
                std::ptr::addr_of_mut!(port) as u64,
                0,
                0,
            ],
        );
        assert_eq!(matched, 2, "both conversions assigned");
        let end = name.iter().position(|b| *b == 0).expect("terminated");
        assert_eq!(&name[..end], b"PORT");
        assert_eq!(port, 2121);
    }

    /// A literal in the format must match, and stops the scan where it does not.
    #[test]
    fn a_literal_that_does_not_match_stops_the_scan() {
        let mut first = 0_i32;
        let mut second = 0_i32;
        let matched = call(
            "sscanf",
            [
                c"12-34".as_ptr() as u64,
                c"%d,%d".as_ptr() as u64,
                std::ptr::addr_of_mut!(first) as u64,
                std::ptr::addr_of_mut!(second) as u64,
                0,
                0,
            ],
        );
        assert_eq!(
            matched, 1,
            "the first was assigned, the comma was not there"
        );
        assert_eq!(first, 12);
        assert_eq!(second, 0, "and the second was left alone");
    }

    /// **The register wall, refused rather than half-performed.**
    ///
    /// Five conversions need five destinations and only four arrive; assigning four and
    /// answering four would tell a caller its first four are good when the arguments may have
    /// been misaligned from the start.
    #[test]
    fn a_format_needing_more_destinations_than_arrived_is_refused() {
        let mut values = [0_i32; 4];
        let matched = call(
            "sscanf",
            [
                c"1,2,3,4,5".as_ptr() as u64,
                c"%d,%d,%d,%d,%d".as_ptr() as u64,
                std::ptr::addr_of_mut!(values[0]) as u64,
                std::ptr::addr_of_mut!(values[1]) as u64,
                std::ptr::addr_of_mut!(values[2]) as u64,
                std::ptr::addr_of_mut!(values[3]) as u64,
            ],
        );
        assert_eq!(matched, 0, "refused entirely");
    }

    /// A suppressed conversion consumes input and assigns nothing.
    #[test]
    fn a_suppressed_conversion_takes_no_destination() {
        let mut wanted = 0_i32;
        let matched = call(
            "sscanf",
            [
                c"skip 77".as_ptr() as u64,
                c"%*s %d".as_ptr() as u64,
                std::ptr::addr_of_mut!(wanted) as u64,
                0,
                0,
                0,
            ],
        );
        assert_eq!(matched, 1);
        assert_eq!(wanted, 77);
    }

    /// A `struct tm`, as `include/time.h` lays one out.
    fn a_time() -> [i32; 9] {
        // 2026-08-29 21:47:05, a Saturday, day 240 of the year.
        [5, 47, 21, 29, 7, 126, 6, 240, 0]
    }

    #[test]
    fn a_timestamp_renders_the_fields_the_structure_holds() {
        let tm = a_time();
        let mut out = [0_u8; 64];
        let written = call(
            "strftime",
            [
                out.as_mut_ptr() as u64,
                out.len() as u64,
                c"%Y-%m-%d %H:%M:%S".as_ptr() as u64,
                tm.as_ptr() as u64,
                0,
                0,
            ],
        );
        assert_eq!(written, 19);
        assert_eq!(&out[..19], b"2026-08-29 21:47:05");
    }

    /// The forms a directory listing uses.
    #[test]
    fn the_listing_forms_render() {
        let tm = a_time();
        let mut out = [0_u8; 64];
        let written = call(
            "strftime",
            [
                out.as_mut_ptr() as u64,
                out.len() as u64,
                c"%b %e %R %a".as_ptr() as u64,
                tm.as_ptr() as u64,
                0,
                0,
            ],
        );
        let end = usize::try_from(written).expect("a length");
        assert_eq!(&out[..end], b"Aug 29 21:47 Sat");
    }

    /// **A conversion it cannot render stops the whole thing.**
    ///
    /// A half-rendered timestamp is a wrong date rather than a short one, and a caller that
    /// prints it has no way to tell.
    #[test]
    fn a_conversion_it_cannot_render_answers_nothing() {
        let tm = a_time();
        let mut out = [0xAA_u8; 64];
        assert_eq!(
            call(
                "strftime",
                [
                    out.as_mut_ptr() as u64,
                    out.len() as u64,
                    c"%Y %Z".as_ptr() as u64,
                    tm.as_ptr() as u64,
                    0,
                    0
                ]
            ),
            0,
            "a timezone is not something this has"
        );
        assert_eq!(out[0], 0xAA, "and nothing was written");
    }

    /// A result that does not fit answers zero, as the interface says.
    #[test]
    fn a_destination_too_small_answers_nothing() {
        let tm = a_time();
        let mut out = [0_u8; 4];
        assert_eq!(
            call(
                "strftime",
                [
                    out.as_mut_ptr() as u64,
                    out.len() as u64,
                    c"%Y-%m-%d".as_ptr() as u64,
                    tm.as_ptr() as u64,
                    0,
                    0
                ]
            ),
            0
        );
    }
}
