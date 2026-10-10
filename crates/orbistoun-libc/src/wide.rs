//! Wide formatted output: `vswprintf` and `wprintf`.
//!
//! The target's `wchar_t` is 16-bit (see `cstring::wcslen`) and its only locale is the C locale,
//! where a byte is the wide character of the same value (FreeBSD `lib/libc/locale/none.c`). A wide
//! format is walked here a character at a time: literal characters pass through untouched, `%s`,
//! `%ls`, `%c` and `%lc` are rendered here so a character above 0xff survives, and every other
//! conversion is handed to the byte renderer, which takes its arguments from the same source and
//! whose ASCII output widens one-to-one.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

use super::{
    Arguments, Count, FORMAT_CALLS, FormatFault, Justify, Registers, c_len, count_argument,
    followable, note_fault, ptr, read_specifier, render_with, varargs,
};

/// The most wide characters read from a guest wide string, matching `wcslen`'s bound.
const MAX_WIDE: usize = 16 * 1024 * 1024;

/// Reads a guest wide string, at most `limit` characters, stopping at its terminator.
fn read_wide(address: u64, limit: usize) -> Vec<u16> {
    let mut out = Vec::new();
    while out.len() < limit {
        let at = address.wrapping_add(out.len() as u64 * 2);
        let Ok(at) = usize::try_from(at) else {
            break;
        };
        // SAFETY: a guest-supplied wide string under the identity mapping, read one character at
        // a time so the scan cannot overrun a mapping by more than it reads.
        let unit =
            unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u16>(at)) };
        if unit == 0 {
            break;
        }
        out.push(unit);
    }
    out
}

/// Whether `unit` can sit between a `%` and its conversion letter: a flag, a digit, the
/// precision's point, `*`, or a length modifier.
fn in_specifier(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|b| b"-+ #0123456789.*hlLqjzt".contains(&b))
}

/// Renders a wide format against an argument source, refusing rather than rendering part of it
/// (D183), as the byte renderer does.
fn render_wide(format: &[u16], args: &mut impl Arguments) -> Result<Vec<u16>, FormatFault> {
    let mut out = Vec::with_capacity(format.len());
    let mut at = 0;
    while let Some(&unit) = format.get(at) {
        at += 1;
        if unit != u16::from(b'%') {
            out.push(unit);
            continue;
        }
        let start = at;
        while format.get(at).copied().is_some_and(in_specifier) {
            at += 1;
        }
        let Some(&letter) = format.get(at) else {
            return Err(FormatFault::Unsupported('%'));
        };
        at += 1;
        let Ok(conversion) = u8::try_from(letter) else {
            return Err(FormatFault::Unsupported(
                char::from_u32(u32::from(letter)).unwrap_or(char::REPLACEMENT_CHARACTER),
            ));
        };
        // Every unit before the letter passed `in_specifier`, so each is ASCII.
        let mut specifier: Vec<u8> = format[start..at - 1].iter().map(|&u| u as u8).collect();
        specifier.push(conversion);
        if matches!(conversion, b's' | b'c') {
            text_conversion(&specifier, args, &mut out)?;
        } else {
            let mut piece = vec![b'%'];
            piece.extend_from_slice(&specifier);
            out.extend(render_with(&piece, args)?.into_iter().map(u16::from));
        }
    }
    Ok(out)
}

/// `%s`, `%ls`, `%c` or `%lc`: the text, cut to the precision and padded with spaces to the width.
/// Without `l` the argument is narrow and each byte widens; with it, wide characters pass as
/// they are.
fn text_conversion(
    specifier: &[u8],
    args: &mut impl Arguments,
    out: &mut Vec<u16>,
) -> Result<(), FormatFault> {
    let mut chars = specifier.iter().copied().peekable();
    let found = read_specifier(&mut chars);
    let conversion = chars.next().unwrap_or(b's');
    let mut justify = found.justify;
    let width = match found.width {
        Count::Given(width) => width,
        Count::Argument => {
            let given = count_argument(args)?;
            if given < 0 {
                justify = Justify::Left;
            }
            usize::try_from(given.unsigned_abs()).unwrap_or(usize::MAX)
        }
    };
    let precision = match found.precision {
        None => None,
        Some(Count::Given(precision)) => Some(precision),
        Some(Count::Argument) => usize::try_from(count_argument(args)?).ok(),
    };
    let value = args.next_integer().ok_or(FormatFault::OutOfArguments)?;
    let wide = found.width_bits == 64;
    let text: Vec<u16> = match (conversion, wide) {
        (b'c', true) => vec![value as u16],
        (b'c', false) => vec![u16::from(value as u8)],
        _ if value == 0 => "(null)".encode_utf16().collect(),
        _ if !followable(value) => "(bad pointer)".encode_utf16().collect(),
        (_, true) => read_wide(value, precision.unwrap_or(MAX_WIDE)),
        (_, false) => {
            // SAFETY: a guest-supplied string under the identity mapping, bounded.
            let len = unsafe { c_len(value) };
            let len = precision.map_or(len, |p| len.min(p));
            // SAFETY: `c_len` established `len` readable bytes from `value`.
            let bytes = unsafe { std::slice::from_raw_parts(ptr(value).cast_const(), len) };
            bytes.iter().copied().map(u16::from).collect()
        }
    };
    let pad = width.saturating_sub(text.len());
    let spaces = std::iter::repeat_n(u16::from(b' '), pad);
    if justify == Justify::Left {
        out.extend_from_slice(&text);
        out.extend(spaces);
    } else {
        out.extend(spaces);
        out.extend_from_slice(&text);
    }
    Ok(())
}

/// `vswprintf(ws, n, format, ap)` - ISO/IEC 9899 7.29.2.7: the rendering written into `ws` and
/// terminated, and its length in wide characters. When `n` or more would be needed, or the format
/// cannot be honoured, the answer is negative; `ws` then holds as much as fits, terminated, as
/// `vsnprintf`'s does (which characters remain is unspecified by the standard).
fn vswprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;

    let (ws, n, format, ap) = (args[0], args[1], args[2], args[3]);
    FORMAT_CALLS.fetch_add(1, Relaxed);
    let room = usize::try_from(n).unwrap_or(usize::MAX);
    let rendered = if format == 0 {
        Err(FormatFault::Unsupported('\0'))
    } else {
        // SAFETY: a guest-supplied `va_list` under the identity mapping; a null one is answered
        // without being dereferenced.
        match unsafe { varargs::VaList::read(ap) } {
            Some(mut list) => render_wide(&read_wide(format, MAX_WIDE), &mut list),
            None => Err(FormatFault::OutOfArguments),
        }
    };
    let (text, answer) = match rendered {
        Ok(text) if text.len() < room => {
            let length = text.len() as u64;
            (text, length)
        }
        Ok(text) => (text, u64::MAX),
        Err(fault) => {
            note_fault(fault);
            (Vec::new(), u64::MAX)
        }
    };
    if ws != 0 && room > 0 {
        let kept = text.len().min(room - 1);
        let mut bytes: Vec<u8> = text[..kept].iter().flat_map(|u| u.to_le_bytes()).collect();
        bytes.extend_from_slice(&[0, 0]);
        // SAFETY: `kept + 1` characters is at most `n`, the size in wide characters the guest
        // declared for `ws`.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr(ws), bytes.len()) };
    }
    answer
}

/// `wprintf(format, ...)` - ISO/IEC 9899 7.29.2.11: the rendering written to the standard output
/// as bytes, each character narrowed as the C locale narrows it, and the number of wide characters
/// answered. A character above 0xff has no byte in that locale: nothing is written and the answer
/// is negative with `errno` `EILSEQ`, as FreeBSD's `_none_wcrtomb` refuses it.
fn wprintf(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;
    use std::sync::atomic::Ordering::Relaxed;

    FORMAT_CALLS.fetch_add(1, Relaxed);
    if args[0] == 0 {
        note_fault(FormatFault::Unsupported('\0'));
        return u64::MAX;
    }
    let format = read_wide(args[0], MAX_WIDE);
    let floats = orbistoun_thunk::float_arguments().unwrap_or_default();
    let mut source = Registers {
        values: &args[1..],
        taken: 0,
        floats: &floats,
        floats_taken: 0,
        stacked: 0,
    };
    let text = match render_wide(&format, &mut source) {
        Ok(text) => text,
        Err(fault) => {
            note_fault(fault);
            return u64::MAX;
        }
    };
    let Ok(bytes) = text
        .iter()
        .map(|&unit| u8::try_from(unit))
        .collect::<Result<Vec<u8>, _>>()
    else {
        let eilseq =
            orbistoun_hle::constants::abi_constant("errno", "EILSEQ").expect("EILSEQ is harvested");
        orbistoun_core::errno::set(eilseq as i32);
        return u64::MAX;
    };
    orbistoun_core::said::note(&bytes);
    // Guest output, written where `printf` writes it.
    let mut err = std::io::stderr();
    let _ = err.write_all(&bytes);
    let _ = err.flush();
    text.len() as u64
}

/// The wide formatted-output functions this module answers.
pub(crate) fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("vswprintf", vswprintf), ("wprintf", wprintf)]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// A terminated wide string from text.
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    /// A `va_list`, its register area and its overflow area, kept alive together.
    type VaListParts = (Box<[u64; 3]>, Box<[u64; 6]>, Box<[u64; 2]>);

    /// A System V `va_list` whose register half holds `words`, and the areas it points into.
    fn va_list(words: &[u64]) -> VaListParts {
        let mut gp = Box::new([0_u64; 6]);
        gp[..words.len()].copy_from_slice(words);
        let overflow = Box::new([0_u64; 2]);
        let mut list = Box::new([0_u64; 3]);
        // gp_offset 0 and fp_offset 48 in the first word, then the overflow and register areas.
        list[0] = 48 << 32;
        list[1] = overflow.as_ptr() as u64;
        list[2] = gp.as_ptr() as u64;
        (list, gp, overflow)
    }

    fn call(f: fn(&[u64; GUEST_ARG_REGISTERS]) -> u64, args: [u64; 4]) -> u64 {
        let mut regs = [0_u64; GUEST_ARG_REGISTERS];
        regs[..4].copy_from_slice(&args);
        f(&regs)
    }

    /// Literal characters above 0xff pass through, `%ls` keeps its wide characters, `%s`
    /// widens its bytes, and a number is rendered as the byte renderer renders it.
    #[test]
    fn vswprintf_renders_wide_and_narrow_arguments() {
        let format = wide("\u{3042}%ls|%s|%-4d|%lc");
        let argument = wide("\u{263a}x");
        let narrow = b"ab\0";
        let (list, _gp, _overflow) =
            va_list(&[argument.as_ptr() as u64, narrow.as_ptr() as u64, 7, 0x263b]);
        let mut out = [0xAAAA_u16; 32];
        let answer = call(
            super::vswprintf,
            [
                out.as_mut_ptr() as u64,
                32,
                format.as_ptr() as u64,
                list.as_ptr() as u64,
            ],
        );
        let expected: Vec<u16> = "\u{3042}\u{263a}x|ab|7   |\u{263b}"
            .encode_utf16()
            .collect();
        assert_eq!(answer, expected.len() as u64);
        assert_eq!(&out[..expected.len()], &expected[..]);
        assert_eq!(out[expected.len()], 0, "terminated");
    }

    /// A rendering that needs `n` or more characters answers negative, keeping what fits.
    #[test]
    fn vswprintf_answers_negative_when_n_is_too_small() {
        let format = wide("abcd");
        let (list, _gp, _overflow) = va_list(&[]);
        let mut out = [0xAAAA_u16; 8];
        let answer = call(
            super::vswprintf,
            [
                out.as_mut_ptr() as u64,
                4,
                format.as_ptr() as u64,
                list.as_ptr() as u64,
            ],
        );
        assert_eq!(answer as i32, -1);
        assert_eq!(&out[..5], &[0x61, 0x62, 0x63, 0, 0xAAAA]);
    }
}
