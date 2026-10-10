//! Reading a formatted string, and writing a formatted time.
//!
//! `sscanf` receives its destinations in registers: six arguments, two spent on the string
//! and the format, so four conversions can be assigned. A format that needs more is refused
//! entirely, since a partial count tells a caller its first conversions are good when the
//! arguments may be misaligned. `strftime` reads the nine leading `int`s of FreeBSD's
//! `struct tm` (`include/time.h`), at offsets 0 to 32 in four-byte steps.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

use crate::{c_len, ptr};

/// Conversions this can assign, being the argument registers less the two fixed ones.
const ASSIGNABLE: usize = GUEST_ARG_REGISTERS - 2;

/// What `sscanf` answers when the input ended before anything matched.
///
/// `EOF`, negative one widened; zero means the input was there and did not match.
const NO_INPUT: u64 = -1_i64 as u64;

/// Reads a NUL-terminated guest string as bytes.
///
/// # Safety
///
/// `address` must name a NUL-terminated string in guest memory, as every string function
/// here requires under the identity mapping.
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
    /// A floating-point number, the `strtod` subject sequence: `%f %e %g %a` and their capitals.
    Float,
    /// `%n`: how much input has been read, stored without counting as an assignment.
    Count,
}

impl Wanted {
    /// How many bytes the destination holds, for the numeric kinds.
    const fn width(self, long: bool) -> usize {
        match self {
            Self::Integer(_) | Self::Unsigned(_) | Self::Float | Self::Count if long => 8,
            Self::Integer(_) | Self::Unsigned(_) | Self::Float | Self::Count => 4,
            Self::String | Self::Character => 1,
        }
    }
}

/// `sscanf(input, format, ...)`.
///
/// Supports `%d`, `%i`, `%u`, `%x`, `%o`, `%s`, `%c`, `%%`, the floating-point conversions
/// `%f %e %g %a` (and capitals) into a `float` or, with `l`, a `double`, `%n`, a maximum field
/// width and the assignment-suppressing `*`. Whitespace in the format matches any run of whitespace; any
/// other character must match itself. Answers how many conversions were assigned, or `EOF`
/// when the input ran out before the first one.
///
/// Reference: ISO C `sscanf`; POSIX.1-2008 `sscanf(3)`.
fn sscanf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    scan(args, false)
}

/// `sscanf_s(input, format, ...)` - C11 K.3.5.3.4: [`sscanf`], with each `%s` and `%c`
/// destination followed by its size. A string that does not fit with its terminator, or a
/// character given no room, is a matching failure: a non-empty destination is left empty and
/// the scan ends there (K.3.5.3.2).
fn sscanf_s(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    scan(args, true)
}

/// The scanner behind both: `sized` is whether `%s` and `%c` take a size.
fn scan(args: &[u64; GUEST_ARG_REGISTERS], sized: bool) -> u64 {
    // SAFETY: guest-supplied strings under the identity mapping.
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
        let (width, long) = width_and_length(&mut format);

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

        // A conversion this cannot read stops the scan: carrying on would assign the next argument
        // from the wrong place.
        let Some(wanted) = wanted_for(conversion) else {
            return assigned;
        };

        // A destination past the sixth argument was passed on the stack, which the trampoline does
        // not reach, so the whole call is refused.
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
        let room = if sized && !suppress && matches!(wanted, Wanted::String | Wanted::Character) {
            let Some(size) = destinations.next() else {
                return 0;
            };
            Some(usize::try_from(size).unwrap_or(usize::MAX))
        } else {
            None
        };

        let taken = match wanted {
            // Reads nothing and is not an assignment (ISO C 7.21.6.2p12).
            Wanted::Count => {
                if !suppress {
                    let count = at as i64;
                    if long {
                        write_bytes(destination, &count.to_le_bytes());
                    } else {
                        write_bytes(destination, &(count as i32).to_le_bytes());
                    }
                }
                continue;
            }
            Wanted::Float => scan_float(input, at, width, destination, suppress, long),
            Wanted::Character if room == Some(0) => None,
            Wanted::Character => scan_character(input, at, destination, suppress),
            Wanted::String => scan_string(input, at, width, destination, suppress, room),
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

/// A conversion's maximum field width (zero for none) and whether its length modifiers include
/// `l`, consumed from the format.
fn width_and_length(format: &mut std::iter::Peekable<impl Iterator<Item = u8>>) -> (usize, bool) {
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
    (width, long)
}

/// What a conversion letter reads, or `None` for one this does not.
const fn wanted_for(conversion: u8) -> Option<Wanted> {
    Some(match conversion {
        b'd' | b'i' => Wanted::Integer(10),
        b'u' => Wanted::Unsigned(10),
        b'x' | b'X' => Wanted::Unsigned(16),
        b'o' => Wanted::Unsigned(8),
        b's' => Wanted::String,
        b'c' => Wanted::Character,
        b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => Wanted::Float,
        b'n' => Wanted::Count,
        _ => return None,
    })
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
    room: Option<usize>,
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
    if let Some(room) = room
        && end - at >= room
    {
        if room > 0 {
            write_bytes(destination, &[0]);
        }
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
    // Every numeric conversion skips leading whitespace, the standard's rule and not the same
    // as a space in the format.
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

/// Reads one floating-point number, answering how much input it took: the `strtod` subject
/// sequence - a sign, digits with an optional point, an optional exponent; or `inf`, `infinity`,
/// `nan` - stored as a `float`, or a `double` when `double`.
///
/// ISO C 7.21.6.2p9: the input item is the longest run that is a prefix of such a sequence, and an
/// item that is not one whole is a matching failure, so `1e` fails where `strtod` would take the
/// `1`. A hexadecimal float is not read; the scan stops there, as for any conversion this cannot
/// carry out.
fn scan_float(
    input: &[u8],
    at: usize,
    width: usize,
    destination: u64,
    suppress: bool,
    double: bool,
) -> Option<usize> {
    let mut cursor = at;
    while input.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    let start = cursor;
    let end = if width > 0 {
        input.len().min(start.saturating_add(width))
    } else {
        input.len()
    };
    let byte = |index: usize| (index < end).then(|| input[index].to_ascii_lowercase());
    if matches!(byte(cursor), Some(b'-' | b'+')) {
        cursor += 1;
    }
    // A word: `inf`, `infinity` or `nan`, matched whole or not at all.
    let word = |cursor: usize, word: &[u8]| {
        (0..word.len())
            .take_while(|&step| byte(cursor + step) == Some(word[step]))
            .count()
    };
    if matches!(byte(cursor), Some(b'i' | b'n')) {
        let (stem, longer): (&[u8], &[u8]) = if byte(cursor) == Some(b'i') {
            (b"inf", b"infinity")
        } else {
            (b"nan", b"nan")
        };
        if word(cursor, stem) != stem.len() {
            return None;
        }
        let matched = word(cursor, longer);
        let taken = if matched == longer.len() {
            longer.len()
        } else if matched == stem.len() {
            stem.len()
        } else {
            return None;
        };
        cursor += taken;
    } else {
        if byte(cursor) == Some(b'0') && byte(cursor + 1) == Some(b'x') {
            return None;
        }
        let digits = |mut cursor: usize| {
            while byte(cursor).is_some_and(|b| b.is_ascii_digit()) {
                cursor += 1;
            }
            cursor
        };
        let whole = digits(cursor);
        let mut after = whole;
        let mut fraction = 0;
        if byte(after) == Some(b'.') {
            let past = digits(after + 1);
            fraction = past - (after + 1);
            after = past;
        }
        if whole == cursor && fraction == 0 {
            return None;
        }
        cursor = after;
        if byte(cursor) == Some(b'e') {
            let mut exponent = cursor + 1;
            if matches!(byte(exponent), Some(b'-' | b'+')) {
                exponent += 1;
            }
            let past = digits(exponent);
            if past == exponent {
                return None;
            }
            cursor = past;
        }
    }
    let text = std::str::from_utf8(&input[start..cursor]).ok()?;
    if !suppress {
        if double {
            write_bytes(destination, &text.parse::<f64>().ok()?.to_le_bytes());
        } else {
            write_bytes(destination, &text.parse::<f32>().ok()?.to_le_bytes());
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
    // SAFETY: a guest-supplied destination under the identity mapping, which the guest promised
    // is large enough, as the real call requires.
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
/// Only the nine leading `int`s; the offset and zone name past them are not used.
fn read_tm(address: u64) -> Option<[i32; 9]> {
    let at = usize::try_from(address).ok()?;
    if at == 0 {
        return None;
    }
    let base = std::ptr::with_exposed_provenance::<i32>(at);
    let mut out = [0_i32; 9];
    for (index, field) in out.iter_mut().enumerate() {
        // SAFETY: a guest-supplied `struct tm` under the identity mapping, whose first nine fields
        // are `int` per `include/time.h`.
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
/// Supports `%Y %m %d %H %M %S %y %j %b %B %a %A %p %e %F %T %D %R %n %t %%` and the forms
/// below. A conversion it cannot render answers zero, as the interface requires when the
/// result does not fit: a half-rendered timestamp is a wrong date. No timezone or locale is
/// modelled, so `%Z` and `%z` stop it and the names are the C locale's.
///
/// Reference: ISO C `strftime`; POSIX.1-2008 `strftime(3)`; `struct tm` from `include/time.h`.
fn strftime(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, max, format, tm) = (args[0], args[1] as usize, args[2], args[3]);
    // SAFETY: a guest-supplied string under the identity mapping.
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
            // Space-padded rather than zero-padded, the difference between `%e` and `%d`.
            b'e' => format!("{mday:2}"),
            b'H' => format!("{hour:02}"),
            b'M' => format!("{min:02}"),
            b'S' => format!("{sec:02}"),
            b'j' => format!("{:03}", yday + 1),
            b'p' => (if hour < 12 { "AM" } else { "PM" }).to_owned(),
            // An index the table does not hold is a broken structure, and stops the rendering
            // rather than naming a day.
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
            // `%X` is the C-locale time, `%T`; `%x` the C-locale date, `%D`.
            b'T' | b'X' => format!("{hour:02}:{min:02}:{sec:02}"),
            b'R' => format!("{hour:02}:{min:02}"),
            b'D' | b'x' => format!(
                "{:02}/{:02}/{:02}",
                mon + 1,
                mday,
                (year + 1900).rem_euclid(100)
            ),
            // The twelve-hour clock. Midnight and noon both read as twelve, not zero.
            b'I' => {
                let h = if hour % 12 == 0 { 12 } else { hour % 12 };
                format!("{h:02}")
            }
            // `%r` is the twelve-hour time in full: `%I:%M:%S %p`.
            b'r' => {
                let h = if hour % 12 == 0 { 12 } else { hour % 12 };
                let meridiem = if hour < 12 { "AM" } else { "PM" };
                format!("{h:02}:{min:02}:{sec:02} {meridiem}")
            }
            // Weekday as a number: `%w` has Sunday at zero, `%u` (ISO) has Monday at one and
            // Sunday at seven.
            b'w' => format!("{wday}"),
            b'u' => format!("{}", if wday == 0 { 7 } else { wday }),
            // The century - the year's first two digits.
            b'C' => format!("{:02}", (year + 1900).div_euclid(100)),
            // `%c` is the C-locale date-and-time, `%a %b %e %H:%M:%S %Y`.
            b'c' => {
                // The day and month names refuse a broken structure, as `%a` and `%b` do.
                let (Some(day), Some(month)) = (
                    name(&DAYS, wday).and_then(|day| day.get(..3)),
                    name(&MONTHS, mon).and_then(|month| month.get(..3)),
                ) else {
                    return 0;
                };
                format!(
                    "{day} {month} {mday:2} {hour:02}:{min:02}:{sec:02} {}",
                    year + 1900
                )
            }
            // Anything else (a zone, a week number, a locale modifier) stops it.
            _ => return 0,
        };
        out.extend_from_slice(rendered.as_bytes());
    }

    // One byte for the terminator.
    if out.len() + 1 > max {
        return 0;
    }
    out.push(0);
    write_bytes(dest, &out);
    (out.len() - 1) as u64
}

/// A name from a table, by an index a guest supplied.
///
/// [`None`] rather than a wrap, so a broken `tm_wday` is not rendered as a real day.
fn name(table: &[&'static str], index: i32) -> Option<&'static str> {
    table.get(usize::try_from(index).ok()?).copied()
}

/// Implementations this module provides, by symbol name.
pub(crate) fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sscanf", sscanf),
        ("sscanf_s", sscanf_s),
        ("strftime", strftime),
    ]
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

    /// `sscanf_s` takes a size after each `%s` and `%c` destination; a string that does not
    /// fit with its terminator empties the destination and ends the scan (C11 K.3.5.3.2).
    #[test]
    fn sscanf_s_takes_a_size_after_each_string_destination() {
        let mut port = 0_i32;
        let mut name = [0xAA_u8; 8];
        let matched = call(
            "sscanf_s",
            [
                c"PORT 2121".as_ptr() as u64,
                c"%s %d".as_ptr() as u64,
                name.as_mut_ptr() as u64,
                5,
                std::ptr::addr_of_mut!(port) as u64,
                0,
            ],
        );
        assert_eq!(matched, 2, "both conversions assigned");
        assert_eq!(&name[..5], b"PORT\0");
        assert_eq!(port, 2121);

        let mut letter = 0_u8;
        let matched = call(
            "sscanf_s",
            [
                c"x".as_ptr() as u64,
                c"%c".as_ptr() as u64,
                std::ptr::addr_of_mut!(letter) as u64,
                1,
                0,
                0,
            ],
        );
        assert_eq!(matched, 1);
        assert_eq!(letter, b'x');

        let mut small = [0xAA_u8; 8];
        let matched = call(
            "sscanf_s",
            [
                c"PORT 2121".as_ptr() as u64,
                c"%s %d".as_ptr() as u64,
                small.as_mut_ptr() as u64,
                4,
                std::ptr::addr_of_mut!(port) as u64,
                0,
            ],
        );
        assert_eq!(matched, 0, "four characters and a terminator need five");
        assert_eq!(small[0], 0, "the destination is left empty");
        assert_eq!(small[1], 0xAA, "and nothing else written");
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

    /// A format needing more destinations than the registers carry is refused, not partly
    /// assigned.
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

    /// `%lf` and `%n`, as oops-mesa's `strtod` asks for every GLSL float literal it parses
    /// (`src/runtime/libc_absent.c`): the double, and how much input it took.
    #[test]
    fn a_double_and_its_length_come_out_as_strtod_wants_them() {
        let read = |text: &std::ffi::CStr| {
            let mut value = -7.0_f64;
            let mut consumed = -1_i32;
            let matched = call(
                "sscanf",
                [
                    text.as_ptr() as u64,
                    c"%lf%n".as_ptr() as u64,
                    std::ptr::addr_of_mut!(value) as u64,
                    std::ptr::addr_of_mut!(consumed) as u64,
                    0,
                    0,
                ],
            );
            (matched, value, consumed)
        };
        assert_eq!(read(c"1."), (1, 1.0, 2), "a GLSL `1.`");
        assert_eq!(read(c"12.345678;"), (1, 12.345_678, 9));
        assert_eq!(
            read(c"  -2.5e3,"),
            (1, -2500.0, 8),
            "leading space, sign, exponent"
        );
        assert_eq!(read(c".5"), (1, 0.5, 2));
        assert_eq!(read(c"1e+2x"), (1, 100.0, 4));
        assert_eq!(read(c"INFINITY"), (1, f64::INFINITY, 8));
        assert_eq!(read(c"-inf"), (1, f64::NEG_INFINITY, 4));
        let (matched, value, consumed) = read(c"nan");
        assert_eq!((matched, consumed), (1, 3));
        assert!(value.is_nan());
        assert_eq!(
            read(c"abc"),
            (0, -7.0, -1),
            "nothing converts: nothing is written"
        );
        // ISO C 7.21.6.2p9: the input item is the longest prefix of a matching sequence, so `1e`
        // is an item that is not one, a matching failure - unlike `strtod`, which takes the `1`.
        assert_eq!(read(c"1e"), (0, -7.0, -1));
        assert_eq!(read(c"infin"), (0, -7.0, -1));
        // A hexadecimal float is a conversion this does not read, so the scan stops there.
        assert_eq!(read(c"0x1p3"), (0, -7.0, -1));
    }

    /// `%f` stores a `float`, and a field width bounds the item.
    #[test]
    fn a_float_takes_its_width() {
        let mut value = 0.0_f32;
        let mut rest = 0_i32;
        let matched = call(
            "sscanf",
            [
                c"1.2345 9".as_ptr() as u64,
                c"%3f%d".as_ptr() as u64,
                std::ptr::addr_of_mut!(value) as u64,
                std::ptr::addr_of_mut!(rest) as u64,
                0,
                0,
            ],
        );
        assert_eq!(matched, 2);
        assert_eq!(value.to_bits(), 1.2_f32.to_bits());
        assert_eq!(rest, 345, "the digits past the width are the next item's");
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

    /// Renders `format` against `tm` and returns what was written.
    fn rendered(tm: &[i32; 9], format: &std::ffi::CStr) -> String {
        let mut out = [0_u8; 64];
        let written = call(
            "strftime",
            [
                out.as_mut_ptr() as u64,
                out.len() as u64,
                format.as_ptr() as u64,
                tm.as_ptr() as u64,
                0,
                0,
            ],
        );
        let end = usize::try_from(written).expect("a length");
        String::from_utf8(out[..end].to_vec()).expect("ascii")
    }

    /// The twelve-hour clock, the weekday numbers, and the C-locale date and time forms.
    #[test]
    fn the_twelve_hour_and_c_locale_forms_render() {
        let tm = a_time();
        assert_eq!(
            rendered(&tm, c"%I"),
            "09",
            "21:00 is nine on the twelve-hour clock"
        );
        assert_eq!(rendered(&tm, c"%r"), "09:47:05 PM");
        assert_eq!(
            rendered(&tm, c"%w %u"),
            "6 6",
            "Saturday is six on both scales"
        );
        assert_eq!(rendered(&tm, c"%C"), "20", "the century of 2026");
        assert_eq!(rendered(&tm, c"%x %X"), "08/29/26 21:47:05");
        assert_eq!(rendered(&tm, c"%c"), "Sat Aug 29 21:47:05 2026");
    }

    /// Midnight reads as twelve with `AM`, and Sunday is seven under `%u` and zero under `%w`.
    #[test]
    fn midnight_and_sunday_hit_the_clock_and_weekday_edges() {
        // 2026-08-30 00:00:00, the Sunday after the Saturday above.
        let tm = [0, 0, 0, 30, 7, 126, 0, 241, 0];
        assert_eq!(rendered(&tm, c"%I"), "12", "midnight is twelve, not zero");
        assert_eq!(rendered(&tm, c"%r"), "12:00:00 AM");
        assert_eq!(
            rendered(&tm, c"%w %u"),
            "0 7",
            "Sunday: zero one way, seven the ISO way"
        );
    }

    /// A conversion it cannot render stops the whole rendering.
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
