//! Orbistoun run against a published implementation of the same interface.
//!
//! # The oracle available today
//!
//! With no console reachable, a reference C library is the only *live* oracle this project
//! has - and unlike hardware it is unboundedly parallel. `tools/differential/reference.c`
//! records what it did; this rebuilds each call from the inputs it recorded and compares.
//!
//! **Agreement means orbistoun implements the published interface as that library does.** It
//! does not mean the console does. The target's C library is FreeBSD-derived and the
//! reference here is glibc, so the two can legitimately differ - which is why results land at
//! the `differential` tier and why that tier stays probeable (D478, D479).
//!
//! # Same shape as the hardware gate, on purpose
//!
//! Every case is either asserted to agree or listed in [`DIVERGES`] with the reason, so a new
//! reference run arrives as a work list. A case that is neither fails the gate. A red test
//! left red forever is not a queue.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::differential::{Argument, Case, Reference};

/// Cases where orbistoun does not answer what the reference answered, and why.
///
/// **This is the work queue.** Each entry names a real difference; fixing one is deleting a
/// line here and watching the gate keep passing.
const DIVERGES: &[(&str, &str)] = &[];

/// Reads the committed reference runs.
fn references() -> Vec<Reference> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../orbistoun-hle/data/differential");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).expect("the committed reference runs") {
        let path = entry.expect("a directory entry").path();
        if path.extension().is_some_and(|e| e == "txt") {
            let text = std::fs::read_to_string(&path).expect("a reference run");
            out.push(Reference::parse(&text));
        }
    }
    assert!(!out.is_empty(), "no reference run is committed");
    out
}

/// What orbistoun answered, in the three things worth comparing.
struct Answer {
    returned: u64,
    /// Named out-parameters, in the reference's own spelling.
    out: std::collections::BTreeMap<String, String>,
}

/// Runs one case through orbistoun.
///
/// **The calling shape is per function and named explicitly.** A generic "pass the arguments
/// through" cannot know which argument is an out-parameter, and guessing would compare the
/// wrong bytes while looking like it worked. A shape this does not know answers `None`, and
/// the gate reports it rather than skipping quietly.
fn run(case: &Case) -> Option<Answer> {
    // **Which shape each function takes, in one place.** This was seven `if` blocks and grew
    // one every time a shape was added, until the function tripped the length lint twice in a
    // day. A reader asking "how is `memcpy` replayed" now has one list to read rather than a
    // chain to walk, and adding a shape is a line rather than a block.
    let shape: Option<fn(&Case) -> Option<Answer>> = match case.function.as_str() {
        "strtod" | "strtof" => Some(run_strtod),
        "qsort" | "bsearch" => Some(run_with_callback),
        "memmove" | "memset" | "strcpy" | "strcat" | "strncpy" | "strncat" | "memcpy" => {
            Some(run_into_buffer)
        }
        "strchr" | "strrchr" | "memchr" | "strstr" | "strpbrk" => Some(run_search),
        "tolower" | "toupper" => Some(run_ctype),
        "strlen" | "strnlen" | "atoi" | "atol" | "atoll" | "abs" | "labs" | "llabs" => {
            Some(run_scalar)
        }
        "wcslen" | "wcscmp" | "wcsrchr" | "wcsncpy" => Some(run_wide),
        "strdup" | "strndup" => Some(run_duplicate),
        "strftime" => Some(run_strftime),
        "sqrt" | "fabs" | "ceil" | "floor" | "trunc" | "round" => Some(run_math),
        // Single precision takes the same path: the argument is the low half of the
        // float register and the answer comes back zero-extended, so the bit pattern
        // crosses identically at both widths (D534).
        "sqrtf" | "fabsf" | "ceilf" | "floorf" | "truncf" | "roundf" | "nearbyintf" => {
            Some(run_math)
        }
        "sprintf" => Some(run_format),
        "vsnprintf" => Some(run_va_format),
        name if CLASSES.contains(&name) => Some(run_ctype),
        _ => None,
    };
    if let Some(shape) = shape {
        return shape(case);
    }
    let mut out = std::collections::BTreeMap::new();
    let call = orbistoun_service::implementation_named(&case.function)?;
    match (case.function.as_str(), case.arguments.as_slice()) {
        // `fn(nptr, endptr, base)`, whose out-parameter is where it stopped reading.
        ("strtoul" | "strtoull" | "strtol", [Argument::Text(subject), Argument::Signed(base)]) => {
            let text = std::ffi::CString::new(subject.clone()).ok()?;
            let mut end: u64 = 0;
            let returned = call(&args([
                text.as_ptr() as u64,
                std::ptr::from_mut(&mut end) as u64,
                *base as u64,
            ]));
            // Reported as an offset, as the reference does: an address is a fact about one
            // process and an offset is a fact about the function.
            let offset = end.saturating_sub(text.as_ptr() as u64);
            out.insert("end_offset".to_owned(), format!("{offset:#x}"));
            Some(Answer { returned, out })
        }
        // `fn(s, set)`, whose whole answer is the return.
        ("strspn" | "strcspn", [Argument::Text(subject), Argument::Text(set)]) => {
            let subject = std::ffi::CString::new(subject.clone()).ok()?;
            let set = std::ffi::CString::new(set.clone()).ok()?;
            let returned = call(&args([subject.as_ptr() as u64, set.as_ptr() as u64, 0]));
            Some(Answer { returned, out })
        }
        // `strlcpy(dst, src, size)` - the return and the buffer are separate halves of the
        // contract, so both are compared. The window is poisoned the way the reference
        // poisons it, or "did not write here" and "wrote a NUL" would read the same.
        (
            "strlcpy",
            [
                Argument::Text(initial),
                Argument::Text(src),
                Argument::Unsigned(size),
            ],
        ) => {
            let mut buffer = poisoned(initial);
            let src = std::ffi::CString::new(src.clone()).ok()?;
            let returned = call(&args([
                buffer.as_mut_ptr() as u64,
                src.as_ptr() as u64,
                *size,
            ]));
            out.insert("buffer".to_owned(), window(&buffer[..12]));
            Some(Answer { returned, out })
        }
        // `snprintf(buffer, room, "%d", value)` - the return and the buffer disagree by
        // design, so both are compared.
        ("snprintf", [Argument::Unsigned(room), Argument::Signed(value)]) => {
            let mut buffer = [b'@'; 64];
            let format = std::ffi::CString::new("%d").ok()?;
            let returned = call(&[
                buffer.as_mut_ptr() as u64,
                *room,
                format.as_ptr() as u64,
                *value as u64,
                0,
                0,
            ]);
            out.insert("buffer".to_owned(), window(&buffer[..8]));
            Some(Answer { returned, out })
        }
        // A comparison, whose contract is the **sign** and not the value. ISO C says greater
        // than, equal to or less than zero and no more; glibc returns the byte difference and
        // another implementation may return exactly the sign. Asserting the magnitude would
        // report a conforming difference as a bug, so the sign is what is compared.
        ("strcmp" | "strcasecmp", [Argument::Text(a), Argument::Text(b)]) => {
            let (a, b) = (text(a)?, text(b)?);
            let returned = call(&args([a.as_ptr() as u64, b.as_ptr() as u64, 0]));
            out.insert("sign".to_owned(), sign_of(returned));
            Some(Answer { returned: 0, out })
        }
        (
            "strncmp" | "strncasecmp" | "memcmp",
            [Argument::Text(a), Argument::Text(b), Argument::Unsigned(n)],
        ) => {
            let (a, b) = (text(a)?, text(b)?);
            let returned = call(&args([a.as_ptr() as u64, b.as_ptr() as u64, *n]));
            out.insert("sign".to_owned(), sign_of(returned));
            Some(Answer { returned: 0, out })
        }
        _ => None,
    }
}

/// The two that call back into the caller's own code, which is what makes them interesting.
///
/// Orbistoun's comparator is a native `sysv64` function pointer - guest code runs in this
/// process, so a test supplies a real one and the emulator calls it exactly as it would call
/// a guest's. That `qsort/jumbled` comes back sorted is the proof it was called at all: a
/// no-op sort would leave the array as it found it.
fn run_with_callback(case: &Case) -> Option<Answer> {
    let call = orbistoun_service::implementation_named(&case.function)?;
    let mut out = std::collections::BTreeMap::new();
    match (case.function.as_str(), case.arguments.as_slice()) {
        (
            "qsort",
            [
                Argument::Bytes(elements),
                Argument::Unsigned(size),
                Argument::Text(order),
            ],
        ) => {
            let compare = comparator(order)?;
            let mut array = elements.clone();
            let count = if *size == 0 {
                0
            } else {
                array.len() / (*size as usize)
            };
            call(&args_of([
                array.as_mut_ptr() as u64,
                count as u64,
                *size,
                compare,
            ]));
            out.insert("array".to_owned(), window(&array));
            Some(Answer { returned: 0, out })
        }
        (
            "bsearch",
            [
                Argument::Bytes(elements),
                Argument::Unsigned(size),
                Argument::Text(order),
                Argument::Signed(key),
            ],
        ) => {
            let compare = comparator(order)?;
            let mut array = elements.clone();
            let count = if *size == 0 {
                0
            } else {
                array.len() / (*size as usize)
            };
            let wanted = i32::try_from(*key).ok()?.to_le_bytes();
            let found = call(&[
                wanted.as_ptr() as u64,
                array.as_mut_ptr() as u64,
                count as u64,
                *size,
                compare,
                0,
            ]);
            Some(located(found, array.as_ptr() as u64, out))
        }
        _ => None,
    }
}

/// The shapes that answer a pointer into their subject.
///
/// Reduced to found-or-not plus an offset, because an address is a fact about one process and
/// an offset is a fact about the function - the same reasoning the end pointer follows.
/// The eleven character classes, by name.
///
/// One list so the dispatch and the match cannot disagree about which functions are swept -
/// the shape of drift that lets a case stop being compared while the gate still passes.
const CLASSES: &[&str] = &[
    "isalnum", "isalpha", "iscntrl", "isdigit", "isgraph", "islower", "isprint", "ispunct",
    "isspace", "isupper", "isxdigit",
];

/// The ctype family: a class swept over sixty-four code points, or a case mapping at one.
///
/// Split out of [`run`] for the reason the searches and the buffer writers were: the match
/// outgrew the length lint, and "sweeps a range" is a different calling shape from "takes a
/// string and a bound", not merely another arm.
fn run_ctype(case: &Case) -> Option<Answer> {
    let call = orbistoun_service::implementation_named(&case.function)?;
    let out = std::collections::BTreeMap::new();
    match (case.function.as_str(), case.arguments.as_slice()) {
        // Compared as *classified or not*, never as the raw return: the standard promises
        // non-zero, glibc answers a mask out of its table, and comparing those would compare
        // an implementation detail rather than the contract.
        (_, [Argument::Unsigned(half)]) if CLASSES.contains(&case.function.as_str()) => {
            let mut bits: u64 = 0;
            for i in 0..64u64 {
                let code = half.saturating_mul(64).saturating_add(i);
                if call(&args([code, 0, 0])) != 0 {
                    bits |= 1 << i;
                }
            }
            Some(Answer {
                returned: bits,
                out,
            })
        }
        // `tolower`/`toupper` at one code point. The answer is a byte, so it is compared
        // directly rather than as a bit.
        ("tolower" | "toupper", [Argument::Signed(input)]) => {
            let returned = call(&args([*input as u64, 0, 0]));
            Some(Answer { returned, out })
        }
        _ => None,
    }
}

/// The functions whose entire answer is the return value.
///
/// Split from [`run`] for the same reason as the searches and the ctype sweep - the match
/// outgrew the length lint - and because these genuinely share one calling shape, which is
/// what made three of their arms identical bodies clippy was right to object to.
///
/// `strlen` and `memcpy` were uncovered until this was written despite being the two
/// most-called functions in the whole recorded corpus. `atoi` and friends are `strtol` with
/// the error reporting removed, which is exactly why they need their own cases rather than
/// being assumed to follow it.
/// `vsnprintf`, driven through a `va_list` this test builds.
///
/// # Why the list is constructed rather than borrowed
///
/// Orbistoun reads a guest's `va_list` as the System V psABI defines it - a four-field
/// structure naming a register save area and an overflow area - so a caller has to supply one.
/// Rust has no way to hand over a C `va_list`, and it does not need to: the layout is
/// published, and what is compared is what was *rendered*, not how the arguments were arranged
/// to get there.
///
/// **So this builds the simplest valid list**: `gp_offset` at zero, six slots in the save area,
/// and everything past the sixth in the overflow area. That is exactly the arrangement a caller
/// with seven or more integer arguments produces, which is the case the cases care about - an
/// implementation reading only the save area renders six correctly and the seventh from
/// whatever follows it.
///
/// # What this cannot check
///
/// The reference's own list has three named parameters before its variadic ones, so its
/// `gp_offset` starts partway up the save area where this one starts at zero. Both are valid
/// and both must render the same text, which is the property under test - but a bug that only
/// appears at a non-zero starting offset would not be caught here.
fn run_va_format(case: &Case) -> Option<Answer> {
    /// Six integer registers at eight bytes each, per the psABI.
    const SAVE_AREA_BYTES: u32 = 48;

    let call = orbistoun_service::implementation_named("vsnprintf")?;
    let mut out = std::collections::BTreeMap::new();

    let [Argument::Unsigned(room), Argument::Text(format), rest @ ..] = case.arguments.as_slice()
    else {
        return None;
    };
    let values: Vec<u64> = rest
        .iter()
        .map(|a| match a {
            Argument::Signed(v) => Some(*v as u64),
            _ => None,
        })
        .collect::<Option<Vec<u64>>>()?;

    let format = text(format)?;
    let mut buffer = [b'@'; 64];
    // The first six go in the save area; the rest are the overflow, in order.
    let mut save = [0_u64; 6];
    for (slot, value) in save.iter_mut().zip(values.iter()) {
        *slot = *value;
    }
    let mut overflow: Vec<u64> = values.iter().skip(6).copied().collect();
    // Never empty, so the pointer is a real address even when nothing overflows - orbistoun
    // refuses an area pointer that could not be one, and rightly.
    overflow.push(0);

    // gp_offset, fp_offset, overflow_arg_area, reg_save_area - in that order, as the psABI
    // lays them out.
    let list: [u64; 3] = [
        u64::from(0_u32) | (u64::from(SAVE_AREA_BYTES) << 32),
        overflow.as_ptr() as u64,
        save.as_ptr() as u64,
    ];

    let returned = call(&[
        buffer.as_mut_ptr() as u64,
        *room,
        format.as_ptr() as u64,
        list.as_ptr() as u64,
        0,
        0,
    ]);
    out.insert("buffer".to_owned(), window(&buffer[..20]));
    Some(Answer { returned, out })
}

/// `sprintf`, which is its own shape because it is the only unbounded formatter here.
///
/// Split from [`run`] for the reason the searches, the ctype sweep and the scalars were: the
/// match outgrew the length lint. The two arms differ only in whether the one variadic
/// argument is a number or a string, which is exactly what the record carries.
fn run_format(case: &Case) -> Option<Answer> {
    let call = orbistoun_service::implementation_named("sprintf")?;
    let mut out = std::collections::BTreeMap::new();
    // Generous, because nothing bounds `sprintf` - and the poison past what was written is
    // compared, so an overrun shows as a changed byte rather than as a corrupted neighbour.
    let mut buffer = [b'@'; 64];
    let format = match case.arguments.first()? {
        Argument::Text(format) => text(format)?,
        _ => return None,
    };
    let argument = match case.arguments.get(1)? {
        Argument::Signed(value) => *value as u64,
        Argument::Text(subject) => {
            let held = text(subject)?;
            let at = held.as_ptr() as u64;
            // Kept alive until after the call: a `CString` dropped here would leave the
            // formatter reading freed memory, which is the bug this shape invites.
            let returned = call(&[
                buffer.as_mut_ptr() as u64,
                format.as_ptr() as u64,
                at,
                0,
                0,
                0,
            ]);
            drop(held);
            out.insert("buffer".to_owned(), window(&buffer[..20]));
            return Some(Answer { returned, out });
        }
        _ => return None,
    };
    let returned = call(&[
        buffer.as_mut_ptr() as u64,
        format.as_ptr() as u64,
        argument,
        0,
        0,
        0,
    ]);
    out.insert("buffer".to_owned(), window(&buffer[..20]));
    Some(Answer { returned, out })
}

fn run_scalar(case: &Case) -> Option<Answer> {
    let call = orbistoun_service::implementation_named(&case.function)?;
    let out = std::collections::BTreeMap::new();
    match case.arguments.as_slice() {
        // One string in, one number out.
        [Argument::Text(subject)] => {
            let subject = text(subject)?;
            let returned = call(&args([subject.as_ptr() as u64, 0, 0]));
            Some(Answer { returned, out })
        }
        // A string and a bound - `strnlen`, whose bound is the whole function.
        [Argument::Text(subject), Argument::Unsigned(n)] => {
            let subject = text(subject)?;
            let returned = call(&args([subject.as_ptr() as u64, *n, 0]));
            Some(Answer { returned, out })
        }
        // One number in, one out. The reference records the answer at the width the
        // function has, so the comparison is of that width rather than of a sign-extended
        // host value.
        [Argument::Signed(value)] => {
            let returned = call(&args([*value as u64, 0, 0]));
            Some(Answer { returned, out })
        }
        _ => None,
    }
}

fn run_search(case: &Case) -> Option<Answer> {
    let call = orbistoun_service::implementation_named(&case.function)?;
    let out = std::collections::BTreeMap::new();
    match (case.function.as_str(), case.arguments.as_slice()) {
        // A search answering a pointer, reduced to found-or-not plus an offset - an address
        // is a fact about one process, an offset is a fact about the function.
        ("strchr" | "strrchr", [Argument::Text(subject), Argument::Signed(needle)]) => {
            let subject = text(subject)?;
            let found = call(&args([subject.as_ptr() as u64, *needle as u64, 0]));
            Some(located(found, subject.as_ptr() as u64, out))
        }
        (
            "memchr",
            [
                Argument::Text(subject),
                Argument::Signed(needle),
                Argument::Unsigned(n),
            ],
        ) => {
            let subject = text(subject)?;
            let found = call(&args([subject.as_ptr() as u64, *needle as u64, *n]));
            Some(located(found, subject.as_ptr() as u64, out))
        }
        // `strpbrk` answers a pointer to the first character in the set, so it reduces to the
        // same found-or-not plus offset as `strstr`. Beside `strcspn`, which asks the same
        // question and answers a length - they disagree in shape exactly at not-found.
        ("strpbrk", [Argument::Text(subject), Argument::Text(set)]) => {
            let subject = text(subject)?;
            let set = text(set)?;
            let found = call(&args([subject.as_ptr() as u64, set.as_ptr() as u64, 0]));
            Some(located(found, subject.as_ptr() as u64, out))
        }
        ("strstr", [Argument::Text(subject), Argument::Text(needle)]) => {
            let (subject, needle) = (text(subject)?, text(needle)?);
            let found = call(&args([subject.as_ptr() as u64, needle.as_ptr() as u64, 0]));
            Some(located(found, subject.as_ptr() as u64, out))
        }
        _ => None,
    }
}

/// The shapes whose whole answer is the bytes they left behind.
///
/// Every one of these returns a pointer nobody checks and is judged entirely on the window:
/// where the terminator landed, what was padded, and what past it was left alone.
fn run_into_buffer(case: &Case) -> Option<Answer> {
    let call = orbistoun_service::implementation_named(&case.function)?;
    let mut out = std::collections::BTreeMap::new();
    match (case.function.as_str(), case.arguments.as_slice()) {
        // A move within one buffer. **The case a naive copy gets wrong in exactly one
        // direction**: front-to-back is correct when the destination is below the source and
        // corrupts when it is above, because the bytes it is about to read have already been
        // overwritten. The answer is entirely in the window.
        (
            "memmove",
            [
                Argument::Text(initial),
                Argument::Unsigned(dest),
                Argument::Unsigned(from),
                Argument::Unsigned(n),
            ],
        ) => {
            let mut buffer = poisoned(initial);
            let base = buffer.as_mut_ptr() as u64;
            call(&args([base + dest, base + from, *n]));
            out.insert("buffer".to_owned(), window(&buffer[..12]));
            Some(Answer { returned: 0, out })
        }
        (
            "memset",
            [
                Argument::Text(initial),
                Argument::Signed(value),
                Argument::Unsigned(n),
            ],
        ) => {
            let mut buffer = poisoned(initial);
            let base = buffer.as_mut_ptr() as u64;
            call(&args([base, *value as u64, *n]));
            out.insert("buffer".to_owned(), window(&buffer[..12]));
            Some(Answer { returned: 0, out })
        }
        // An unbounded copy or append. What is watched is the terminator and what is left
        // past it - a shim that copies the right characters and forgets the NUL passes every
        // test that reads the result back as a string.
        ("strcpy" | "strcat", [Argument::Text(initial), Argument::Text(src)]) => {
            let mut buffer = poisoned(initial);
            let source = text(src)?;
            call(&args([
                buffer.as_mut_ptr() as u64,
                source.as_ptr() as u64,
                0,
            ]));
            out.insert("buffer".to_owned(), window(&buffer[..12]));
            Some(Answer { returned: 0, out })
        }
        // A bounded copy into a poisoned window. The bytes are the whole answer: `strncpy`
        // pads the remainder with NULs when the source is short and does **not** terminate
        // when it is not, and neither is visible in a return value.
        (
            "strncpy" | "strncat" | "memcpy",
            [
                Argument::Text(initial),
                Argument::Text(src),
                Argument::Unsigned(n),
            ],
        ) => {
            let mut buffer = poisoned(initial);
            let src = text(src)?;
            call(&args([buffer.as_mut_ptr() as u64, src.as_ptr() as u64, *n]));
            out.insert("buffer".to_owned(), window(&buffer[..12]));
            Some(Answer { returned: 0, out })
        }
        _ => None,
    }
}

/// A sixteen-byte window holding `initial`, with the rest left as a pattern.
///
/// The poison is what makes an overrun visible: bytes past the terminator are `@` until
/// something writes them, so a copy that goes too far shows up in the record rather than
/// landing on a zero that was already there.
fn poisoned(initial: &[u8]) -> [u8; 16] {
    let mut buffer = [b'@'; 16];
    buffer[..initial.len()].copy_from_slice(initial);
    buffer[initial.len()] = 0;
    buffer
}

/// The comparison a sorting case names, as an address orbistoun can call.
///
/// **The one place the "there is only one case list" property does not reach.** A comparator
/// is code, and the reference cannot hand its machine code to the checker - so the case names
/// a *semantic* and each side implements it. A name this does not know answers `None`, which
/// the rebuild gate reports rather than passing over: an unrecognised comparator must not
/// become a silently skipped case.
fn comparator(order: &[u8]) -> Option<u64> {
    match order {
        // Through a function *pointer* rather than casting the item, which is what the
        // lint asks for and is also the honest spelling: the address handed over is the one
        // a call would go through.
        b"int32-asc" => {
            let f: extern "sysv64" fn(u64, u64) -> u64 = int32_ascending;
            Some(f as usize as u64)
        }
        _ => None,
    }
}

/// Ascending order over four-byte integers, in the guest's own calling convention.
extern "sysv64" fn int32_ascending(a: u64, b: u64) -> u64 {
    // SAFETY: orbistoun hands the comparator addresses inside the array this test allocated
    // and described, which is the contract the interface has and this test satisfies.
    let x = unsafe { std::ptr::read_unaligned(a as usize as *const i32) };
    // SAFETY: the same, for the second element.
    let y = unsafe { std::ptr::read_unaligned(b as usize as *const i32) };
    // Answered as a C `int` widened to the register, which is what the guest ABI carries.
    let answer = i32::from(x > y) - i32::from(x < y);
    u64::from(answer as u32)
}

/// Pads a call to the register count, for the shapes that take four.
fn args_of(given: [u64; 4]) -> [u64; GUEST_ARG_REGISTERS] {
    let mut out = [0; GUEST_ARG_REGISTERS];
    out[..4].copy_from_slice(&given);
    out
}

/// The one case that answers in a floating-point register, so it comes from the other table.
///
/// Its value is compared as a **bit pattern** rather than as a number, because a decimal
/// rendering would hide exactly the last-place differences worth catching.
fn run_strtod(case: &Case) -> Option<Answer> {
    // **The function is taken from the case, not hard-coded.** `strtof` is the same shape at
    // single precision and shares this path; naming `strtod` here would have run every
    // `strtof` case against the double and passed on the wrong implementation.
    let call = orbistoun_service::float_implementation_named(&case.function)?;
    let [Argument::Text(subject)] = case.arguments.as_slice() else {
        return None;
    };
    let subject = text(subject)?;
    let mut end: u64 = 0;
    let ints = args([
        subject.as_ptr() as u64,
        std::ptr::from_mut(&mut end) as u64,
        0,
    ]);
    let returned = call(&ints, &[0; orbistoun_core::GUEST_FLOAT_REGISTERS]);
    let offset = end.saturating_sub(subject.as_ptr() as u64);
    let mut out = std::collections::BTreeMap::new();
    out.insert("end_offset".to_owned(), format!("{offset:#x}"));
    Some(Answer { returned, out })
}

/// One answer per step, however many steps the sequence has.
///
/// Everything but `strtok` is a sequence of one, so the two shapes meet here rather than at
/// every call site.
fn answers_for(steps: &[&Case]) -> Option<Vec<Answer>> {
    if steps[0].function == "strtok" || steps[0].function == "strtok_r" {
        return run_strtok_sequence(steps);
    }
    if steps.len() != 1 {
        return None;
    }
    run(steps[0]).map(|a| vec![a])
}

/// Replays a whole `strtok` sequence against one buffer.
///
/// # Why this cannot be one case at a time
///
/// `strtok` carries its place between calls, so the second call's answer depends on the
/// first having happened - and it **mutates the subject**, writing a NUL over each delimiter
/// it consumes. Half the contract is therefore invisible in the return values: a shim that
/// answers the right tokens without writing the terminators is wrong in a way only the bytes
/// show, so the buffer is compared too.
///
/// # And why it takes a lock
///
/// Orbistoun keeps the place in a process-wide value, which is **correct** - ISO C says
/// `strtok` is not reentrant and the platform's will not be either. But this file's tests run
/// on several threads, and two sequences interleaved would each see the other's position. The
/// lock is the test being honest about a property of the function rather than a workaround.
fn run_strtok_sequence(steps: &[&Case]) -> Option<Vec<Answer>> {
    /// One sequence at a time, because the place `strtok` keeps is shared by all of them.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// How many interleaved walks one sequence may carry.
    ///
    /// Two, because two is what distinguishes `strtok_r` from `strtok` and a third proves
    /// nothing a second does not. A record asking for more is refused rather than folded into
    /// stream one, which would compare the wrong walk while looking like it worked.
    const STREAMS: usize = 2;

    // Taken from the case: `strtok_r` shares this path and naming `strtok` here would run
    // every reentrant case against the non-reentrant function (the D508 wire).
    let name = steps[0].function.as_str();
    let call = orbistoun_service::implementation_named(name)?;

    // **One buffer and one saved place per stream.** A sequence with no stream argument is
    // stream zero, which is every sequence written before interleaving existed - so the older
    // cases run through exactly the path they always did.
    let mut buffers = [[b'@'; 24]; STREAMS];
    let mut saves = [0_u64; STREAMS];
    let mut used = 1;

    let _held = ONE_AT_A_TIME.lock().ok()?;
    let mut answers = Vec::with_capacity(steps.len());
    for step in steps {
        let (subject, set, stream) = match step.arguments.as_slice() {
            [subject, Argument::Text(set)] => (subject, set, 0),
            [subject, Argument::Text(set), Argument::Unsigned(stream)] => {
                (subject, set, usize::try_from(*stream).ok()?)
            }
            _ => return None,
        };
        if stream >= STREAMS {
            return None;
        }
        used = used.max(stream + 1);
        let delimiters = text(set)?;

        // A `Text` first argument starts that stream's walk and installs its buffer; a `Null`
        // continues it. That is the function's own contract, and it is why the two shapes can
        // be told apart from the record without a flag saying which is which.
        let start = match subject {
            Argument::Text(bytes) => {
                if bytes.len() >= buffers[stream].len() {
                    return None;
                }
                let buffer = &mut buffers[stream];
                *buffer = [b'@'; 24];
                buffer[..bytes.len()].copy_from_slice(bytes);
                buffer[bytes.len()] = 0;
                buffer.as_mut_ptr() as u64
            }
            Argument::Null => 0,
            _ => return None,
        };
        // **The reentrant form keeps its place here, where the caller can see it.** That is the
        // whole difference between the two, so the storage is this test's, one per stream, and
        // is threaded through every step - exactly as a caller must.
        let save_ptr = if name == "strtok_r" {
            std::ptr::from_mut(&mut saves[stream]) as u64
        } else {
            0
        };
        let found = call(&args([start, delimiters.as_ptr() as u64, save_ptr]));
        answers.push(located(
            found,
            buffers[stream].as_ptr() as u64,
            std::collections::BTreeMap::new(),
        ));
    }

    // The buffers, on the last step: `strtok` mutates its subject, and a shim that answers the
    // right tokens without writing the terminators is wrong in a way only the bytes show.
    if let Some(last) = answers.last_mut() {
        if used == 1 {
            last.out
                .insert("buffer".to_owned(), window(&buffers[0][..16]));
        } else {
            for (stream, buffer) in buffers.iter().enumerate().take(used) {
                last.out
                    .insert(format!("buffer{stream}"), window(&buffer[..16]));
            }
        }
    }
    Some(answers)
}

/// Lays a recorded subject out as a NUL-terminated string this process owns.
fn text(bytes: &[u8]) -> Option<std::ffi::CString> {
    std::ffi::CString::new(bytes.to_vec()).ok()
}

/// The sign of a comparison, in the reference's own words.
fn sign_of(returned: u64) -> String {
    // The shim answers in a 64-bit register and the value is a C `int`, so the sign lives in
    // bit 31 rather than bit 63 - reading it as `i64` would call every negative answer
    // positive.
    match returned as u32 as i32 {
        0 => "zero".to_owned(),
        n if n > 0 => "positive".to_owned(),
        _ => "negative".to_owned(),
    }
}

/// Turns a returned pointer into found-or-not plus an offset from the subject.
fn located(found: u64, base: u64, mut out: std::collections::BTreeMap<String, String>) -> Answer {
    if found == 0 {
        return Answer { returned: 0, out };
    }
    out.insert(
        "offset".to_owned(),
        format!("{:#x}", found.saturating_sub(base)),
    );
    Answer { returned: 1, out }
}

/// `strdup` and `strndup`, whose answer is an address and whose contract is the bytes behind it.
///
/// # The read is bounded, and that is not caution
///
/// A `strndup` that does not terminate is one of the bugs here, and reading the copy "until the
/// NUL" would run off the end of exactly the allocation that bug produces - the test would
/// crash instead of reporting, on the input it was written for. So the read stops at a bound.
///
/// # What these cases cannot reliably catch, measured rather than assumed
///
/// **A missing terminator, only sometimes.** The byte after an unterminated copy is whatever
/// the allocator last left there, and it is a zero often enough to matter: breaking the
/// terminator and running three times failed three, three and four cases, and *different* ones
/// each time (D535).
///
/// So what these verify is the **contents** of a copy that is terminated. Termination itself is
/// caught probabilistically, and calling that verified would be the confident wrong answer this
/// whole file exists to avoid. Catching it properly needs the allocation poisoned before the
/// call, which is the reference's technique for buffers it owns and not something a caller of
/// `strdup` can do.
fn run_duplicate(case: &Case) -> Option<Answer> {
    /// Far more than any case copies, and short enough that an unterminated answer is reported
    /// rather than walked.
    const BOUND: usize = 64;

    let call = orbistoun_service::implementation_named(&case.function)?;
    let mut out = std::collections::BTreeMap::new();
    let (subject, count) = match case.arguments.as_slice() {
        [Argument::Text(subject)] => (subject, 0),
        [Argument::Text(subject), Argument::Unsigned(count)] => (subject, *count),
        _ => return None,
    };
    let subject = text(subject)?;
    let answer = call(&args([subject.as_ptr() as u64, count, 0]));
    if answer == 0 {
        return Some(Answer { returned: 0, out });
    }
    let at = usize::try_from(answer).ok()?;
    let mut bytes = Vec::with_capacity(BOUND);
    let mut terminated = false;
    for step in 0..BOUND {
        // SAFETY: the address `strdup`/`strndup` just answered, read one byte at a time and
        // stopped at the first NUL - which every conforming answer has inside the bound.
        let byte = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u8>(at + step)) };
        bytes.push(byte);
        if byte == 0 {
            terminated = true;
            break;
        }
    }
    out.insert(
        "copy".to_owned(),
        if terminated {
            window(&bytes)
        } else {
            "unterminated".to_owned()
        },
    );
    Some(Answer { returned: 1, out })
}

/// The exactly-specified math functions, whose answers are bit patterns.
///
/// # Which ones are here, and why the rest are not
///
/// `sqrt`, `fabs`, `ceil`, `floor`, `trunc` and `round` are **exactly specified** - IEEE-754
/// and ISO C give each input one correct answer, so comparing bit-for-bit states the contract.
///
/// The transcendentals - `sin`, `cos`, `exp`, `log`, `pow` and around forty others, all
/// implemented and all uncompared - are **deliberately absent**. They are permitted to differ
/// in the last place, so a bit comparison would pin orbistoun to *glibc's* libm rather than to
/// any contract. That is the same reason `rand` and `strerror` are out (D532, D533).
///
/// # Why the value is a bit pattern on both sides
///
/// A decimal rendering hides the last-place differences that are the whole point, and it hides
/// the **sign of zero** - which is not a detail: `round(-0.5)` is `-0.0`, and an implementation
/// answering `+0.0` passes every comparison that goes through a string.
///
/// # What this cannot prove
///
/// That the console's libm agrees with glibc on these. It cannot: the claim is that both
/// implement the *specified* answer, and for these six there is one. For anything where the
/// specification permits a range, no differential could settle it and this does not pretend to.
fn run_math(case: &Case) -> Option<Answer> {
    let call = orbistoun_service::float_implementation_named(&case.function)?;
    let [Argument::Unsigned(bits)] = case.arguments.as_slice() else {
        return None;
    };
    let mut floats = [0_u64; orbistoun_core::GUEST_FLOAT_REGISTERS];
    floats[0] = *bits;
    let returned = call(&args([0, 0, 0]), &floats);
    Some(Answer {
        returned,
        out: std::collections::BTreeMap::new(),
    })
}

/// `strftime(dest, room, format, tm)` - a specifier parser, which is the shape that hid two
/// bugs in `sprintf` (D511).
///
/// # What crosses, and what deliberately does not
///
/// Only the **nine `int` fields** of `struct tm`, as a `b:` blob. ISO C fixes their names and
/// order and both sides agree on them; the fields past the ninth are not recorded, because
/// orbistoun does not read them and the conversions that would need them (`%Z`, `%z`) are ones
/// it refuses.
///
/// **The buffer is compared only when the call succeeded.** ISO C leaves the contents
/// unspecified when the result does not fit, so the reference records none there and neither
/// does this - comparing them would be comparing something neither implementation promises.
///
/// # What this cannot prove
///
/// That the console's `struct tm` is laid out this way. Both sides assume the ISO C order, and
/// orbistoun cites FreeBSD's LP64 form where it reads them; if the target differs, every case
/// here agrees and both are wrong together - which is the same limit every differential has.
fn run_strftime(case: &Case) -> Option<Answer> {
    let call = orbistoun_service::implementation_named(&case.function)?;
    let mut out = std::collections::BTreeMap::new();
    let [
        Argument::Unsigned(room),
        Argument::Text(format),
        Argument::Bytes(when),
    ] = case.arguments.as_slice()
    else {
        return None;
    };
    let format = text(format)?;
    let when = when.clone();
    // The same window and the same poison the reference used, so a short render and a
    // half-render are told apart by the bytes rather than by the count alone.
    let mut buffer = [b'@'; 64];
    if *room as usize > buffer.len() {
        return None;
    }
    let returned = call(&args_of([
        buffer.as_mut_ptr() as u64,
        *room,
        format.as_ptr() as u64,
        when.as_ptr() as u64,
    ]));
    if returned > 0 {
        out.insert("buffer".to_owned(), window(&buffer[..32]));
    }
    Some(Answer { returned, out })
}

/// The wide-character family, whose subjects arrive as `b:` blobs.
///
/// # Why the subject is bytes and the offset is elements
///
/// A wide string is element data - `L'A'` is `41 00 00 00` - so it rides in the record as a
/// byte blob and is handed over as one (D529). But `wcsrchr` answers a pointer *into* that
/// array, and the reference records `at - s` in **`wchar_t` units**, because that is what
/// pointer arithmetic on a `wchar_t *` produces. Dividing by four here is not a conversion, it
/// is the same arithmetic on this side.
///
/// # What these cannot prove
///
/// That a wide character is four bytes on the console. Both sides assume it: glibc by its own
/// definition, orbistoun by saying so where it implements them. If the target's `wchar_t` is
/// not four bytes, every case here agrees with the reference and both are wrong together -
/// which no differential can catch, because a differential compares two implementations and
/// not either against hardware.
fn run_wide(case: &Case) -> Option<Answer> {
    /// Bytes per wide character, on both sides of this comparison.
    const WIDE: u64 = 4;

    let call = orbistoun_service::implementation_named(&case.function)?;
    let mut out = std::collections::BTreeMap::new();
    match (case.function.as_str(), case.arguments.as_slice()) {
        ("wcslen", [Argument::Bytes(subject)]) => {
            let subject = subject.clone();
            Some(Answer {
                returned: call(&args([subject.as_ptr() as u64, 0, 0])),
                out,
            })
        }
        ("wcscmp", [Argument::Bytes(a), Argument::Bytes(b)]) => {
            let (a, b) = (a.clone(), b.clone());
            let returned = call(&args([a.as_ptr() as u64, b.as_ptr() as u64, 0]));
            out.insert("sign".to_owned(), sign_of(returned));
            Some(Answer { returned: 0, out })
        }
        ("wcsrchr", [Argument::Bytes(subject), Argument::Unsigned(needle)]) => {
            let subject = subject.clone();
            let base = subject.as_ptr() as u64;
            let found = call(&args([base, *needle, 0]));
            if found == 0 {
                return Some(Answer { returned: 0, out });
            }
            out.insert(
                "offset".to_owned(),
                format!("{:#x}", found.saturating_sub(base) / WIDE),
            );
            Some(Answer { returned: 1, out })
        }
        ("wcsncpy", [Argument::Bytes(source), Argument::Unsigned(count)]) => {
            let source = source.clone();
            // Eight wide characters, poisoned - the same window and the same poison the
            // reference used, so padding and non-termination are both visible rather than
            // inferred from a length.
            let mut destination = [0x40_u8; 8 * WIDE as usize];
            call(&args([
                destination.as_mut_ptr() as u64,
                source.as_ptr() as u64,
                *count,
            ]));
            out.insert("buffer".to_owned(), window(&destination));
            Some(Answer { returned: 0, out })
        }
        _ => None,
    }
}

/// A byte window, in the reference's hex spelling.
fn window(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, b| {
        use std::fmt::Write as _;
        let _ = write!(text, "{b:02x}");
        text
    })
}

/// Pads a call to the register count, since most of these take fewer.
fn args(given: [u64; 3]) -> [u64; GUEST_ARG_REGISTERS] {
    let mut out = [0; GUEST_ARG_REGISTERS];
    out[..3].copy_from_slice(&given);
    out
}

/// Every case runs, and the shapes this does not know are named rather than skipped.
///
/// The failure a silent skip causes: a differential that compares nothing reports agreement,
/// which is the most confident wrong answer available.
#[test]
fn every_recorded_case_can_be_rebuilt() {
    let runs = references();
    let unbuildable: Vec<String> = runs
        .iter()
        .flat_map(Reference::sequences)
        .filter(|(_, steps)| answers_for(steps).is_none())
        .map(|(name, steps)| format!("{name} ({})", steps[0].function))
        .collect();
    assert!(
        unbuildable.is_empty(),
        "recorded by the reference and not rebuildable here: {unbuildable:#?}"
    );
}

/// **Orbistoun answers what the reference answered, or the difference is written down.**
#[test]
fn orbistoun_agrees_with_the_reference_or_says_where_it_does_not() {
    let diverges: std::collections::BTreeMap<&str, &str> = DIVERGES.iter().copied().collect();
    let mut surprises = Vec::new();
    let mut agreed = 0_usize;

    for reference in references() {
        for (_, steps) in reference.sequences() {
            let Some(answers) = answers_for(&steps) else {
                continue;
            };
            for (case, answer) in steps.iter().zip(answers) {
                let mut differences = Vec::new();
                if answer.returned != case.returned {
                    differences.push(format!(
                        "returned {:#x}, {} returned {:#x}",
                        answer.returned, reference.library, case.returned
                    ));
                }
                for (name, expected) in &case.out {
                    match answer.out.get(name) {
                        Some(got) if got == expected => {}
                        Some(got) => {
                            differences.push(format!("{name} is {got}, expected {expected}"));
                        }
                        None => differences.push(format!("{name} was not produced")),
                    }
                }
                match (
                    differences.is_empty(),
                    diverges.contains_key(case.id.as_str()),
                ) {
                    (true, false) => agreed += 1,
                    (true, true) => surprises.push(format!(
                        "{}: listed as diverging and it agrees - delete the line",
                        case.id
                    )),
                    (false, true) => {}
                    (false, false) => {
                        surprises.push(format!("{}: {}", case.id, differences.join("; ")));
                    }
                }
            }
        }
    }
    assert!(
        surprises.is_empty(),
        "differences nobody has written down (or stale entries): {surprises:#?}"
    );

    // **Every rebuilt case is accounted for, not merely "some agreed".**
    //
    // `agreed > 0` was the first version of this, and it is the shape of assertion that lets
    // a differential rot into nothing: a checker that silently stopped comparing sixty of
    // sixty-three cases would still pass it. Counting successes is not checking for failures,
    // so the count is asserted against the case list instead.
    let runs = references();
    let rebuilt: usize = runs
        .iter()
        .flat_map(Reference::sequences)
        .filter_map(|(_, steps)| answers_for(&steps).map(|a| a.len()))
        .sum();
    assert_eq!(
        agreed + diverges.len(),
        rebuilt,
        "{rebuilt} cases were rebuilt, {agreed} agreed and {} are recorded as diverging",
        diverges.len()
    );
}

/// `errno` is not compared, and the reason is recorded rather than left as an omission.
///
/// **Orbistoun keeps no guest `errno`.** The `sem_*` family says so in as many words: this
/// project answers the code directly rather than setting a variable it does not maintain. So
/// the reference's `errno` column is captured - it is a fact about the library and worth
/// having when a guest `errno` exists - and comparing it today would report a difference in
/// every case that sets one, which is a finding already known and recorded, not news.
#[test]
fn the_reference_records_errno_even_though_nothing_compares_it_yet() {
    let runs = references();
    let setting: Vec<&str> = runs
        .iter()
        .flat_map(|r| &r.cases)
        .filter(|c| c.errno != 0)
        .map(|c| c.id.as_str())
        .collect();
    assert!(
        !setting.is_empty(),
        "no case provokes errno, so the column proves nothing - add an overflow case"
    );
}
