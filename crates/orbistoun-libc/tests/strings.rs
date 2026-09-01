//! The memory and string functions, called the way a guest calls them.
//!
//! # Guest memory is host memory
//!
//! The mapping is identity (D014), so a `Vec<u8>` this test owns *is* a guest buffer. That
//! makes the whole family testable without an address space, which is the pattern principle
//! 8 asks for: a pure decision plus a thin effectful wrapper, exercised at the wrapper.
//!
//! `Buf` takes its address from a mutable pointer at construction and hands out the same
//! integer thereafter, so a function writing through it is writing to the allocation this
//! test still owns and can read back.
//!
//! # What these are for
//!
//! Every one of these functions has a contract a plausible implementation gets *nearly*
//! right, and the near-misses are the tests worth having: `strncpy` that does not pad,
//! `strchr` that cannot find the terminator, `strcmp` that stops before it, `strncat` whose
//! limit bounds the wrong string. Each is a one-character edit away from correct and none
//! of them fails loudly.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// A writable guest buffer at a real address.
struct Buf {
    storage: Vec<u8>,
    at: u64,
}

impl Buf {
    /// `size` zero bytes.
    fn zeroed(size: usize) -> Self {
        Self::new(vec![0; size])
    }

    /// A NUL-terminated string with `size` bytes of room around it.
    fn text(s: &str, size: usize) -> Self {
        assert!(s.len() < size, "the terminator needs room too");
        let mut v = vec![0_u8; size];
        v[..s.len()].copy_from_slice(s.as_bytes());
        Self::new(v)
    }

    /// Arbitrary bytes with a terminator appended.
    fn raw(bytes: &[u8]) -> Self {
        let mut v = bytes.to_vec();
        v.push(0);
        Self::new(v)
    }

    /// The address is taken from a **mutable** pointer, so writing through it later is
    /// sound rather than merely working: a pointer derived from a shared reference would
    /// carry no permission to write.
    fn new(mut storage: Vec<u8>) -> Self {
        let at = storage.as_mut_ptr().expose_provenance() as u64;
        Self { storage, at }
    }

    fn at(&self) -> u64 {
        self.at
    }

    /// What a C reader would see: everything up to the first terminator.
    fn as_str(&self) -> &str {
        let end = self
            .storage
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(self.storage.len());
        std::str::from_utf8(&self.storage[..end]).expect("test buffers hold text")
    }

    /// The whole buffer, terminators and padding included.
    fn bytes(&self) -> &[u8] {
        &self.storage
    }
}

/// The implementation registered under `name`.
fn implementation(name: &str) -> GuestFn {
    orbistoun_libc::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(
            || panic!("{name} is not implemented, so nothing can call it"),
            |(_, f)| *f,
        )
}

/// Calls one, poisoning the argument registers it does not use.
fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0xDEAD_BEEF_DEAD_BEEF_u64; GUEST_ARG_REGISTERS];
    for (slot, value) in regs.iter_mut().zip(args) {
        *slot = *value;
    }
    implementation(name)(&regs)
}

// --- memory ------------------------------------------------------------------------------

/// `memset` fills exactly the range asked for and returns its destination.
///
/// The return value is not decoration: callers chain on it, so a function that filled
/// correctly and answered zero would break code that never looked at the buffer.
#[test]
fn memset_fills_exactly_its_range_and_returns_the_destination() {
    let buf = Buf::zeroed(8);
    assert_eq!(call("memset", &[buf.at() + 2, 0xAB, 4]), buf.at() + 2);
    assert_eq!(buf.bytes(), &[0, 0, 0xAB, 0xAB, 0xAB, 0xAB, 0, 0]);
}

/// A count of zero, or a null destination, writes nothing.
#[test]
fn memset_of_nothing_writes_nothing() {
    let buf = Buf::zeroed(4);
    assert_eq!(call("memset", &[buf.at(), 0xFF, 0]), buf.at());
    assert_eq!(buf.bytes(), &[0, 0, 0, 0]);
    assert_eq!(
        call("memset", &[0, 0xFF, 8]),
        0,
        "a null destination is a no-op"
    );
}

/// Only the low byte of the fill value is used, which is what C specifies.
#[test]
fn memset_uses_only_the_low_byte_of_its_value() {
    let buf = Buf::zeroed(3);
    call("memset", &[buf.at(), 0x1234_5641, 3]);
    assert_eq!(buf.bytes(), &[0x41, 0x41, 0x41]);
}

/// `memcpy` copies forwards and answers with its destination.
#[test]
fn memcpy_copies_and_returns_the_destination() {
    let src = Buf::raw(b"abcd");
    let dest = Buf::zeroed(8);
    assert_eq!(call("memcpy", &[dest.at(), src.at(), 4]), dest.at());
    assert_eq!(&dest.bytes()[..4], b"abcd");
    assert_eq!(&dest.bytes()[4..], &[0, 0, 0, 0]);
}

/// Overlapping ranges are handled rather than corrupted.
///
/// A guest that overlaps here has technically broken `memcpy`'s contract, but the stricter
/// primitive would give it silent corruption and being permissive costs nothing. `memmove`
/// makes the same guarantee explicitly, and the two must agree.
#[test]
fn an_overlapping_copy_is_not_corrupted() {
    for name in ["memcpy", "memmove"] {
        let buf = Buf::raw(b"abcdef");
        // Shift the tail one byte to the right, which a forward byte-by-byte copy smears.
        call(name, &[buf.at() + 1, buf.at(), 5]);
        assert_eq!(&buf.bytes()[..6], b"aabcde", "{name} smeared an overlap");
    }
}

/// A null pointer or a zero count copies nothing.
#[test]
fn a_copy_of_nothing_copies_nothing() {
    let src = Buf::raw(b"abcd");
    let dest = Buf::zeroed(4);
    call("memcpy", &[dest.at(), src.at(), 0]);
    assert_eq!(dest.bytes(), &[0, 0, 0, 0]);
    assert_eq!(call("memcpy", &[dest.at(), 0, 4]), dest.at());
    assert_eq!(dest.bytes(), &[0, 0, 0, 0], "a null source copies nothing");
}

/// `memcmp` reports a sign, and it is the sign of the first differing byte.
///
/// Compared as **unsigned** bytes, which is the trap: a signed comparison makes `0x80`
/// sort below `0x01` and reverses the answer for exactly the inputs a text-based test
/// never uses.
#[test]
fn memcmp_compares_bytes_as_unsigned() {
    let low = Buf::raw(&[0x01]);
    let high = Buf::raw(&[0x80]);
    assert!((call("memcmp", &[low.at(), high.at(), 1]) as i64) < 0);
    assert!((call("memcmp", &[high.at(), low.at(), 1]) as i64) > 0);

    let same = Buf::raw(&[0x80]);
    assert_eq!(call("memcmp", &[high.at(), same.at(), 1]), 0);
}

/// A comparison stops at the count, and only the bytes inside it matter.
#[test]
fn memcmp_looks_at_no_more_than_its_count() {
    let a = Buf::raw(b"abcX");
    let b = Buf::raw(b"abcY");
    assert_eq!(call("memcmp", &[a.at(), b.at(), 3]), 0);
    assert_ne!(call("memcmp", &[a.at(), b.at(), 4]), 0);
    assert_eq!(call("memcmp", &[a.at(), b.at(), 0]), 0);
    assert_eq!(
        call("memcmp", &[0, b.at(), 4]),
        0,
        "a null side compares equal"
    );
}

/// `memchr` returns an address, not an index, and null when the byte is absent.
#[test]
fn memchr_returns_the_address_of_the_byte() {
    let buf = Buf::raw(b"abcd");
    assert_eq!(
        call("memchr", &[buf.at(), u64::from(b'c'), 4]),
        buf.at() + 2
    );
    assert_eq!(call("memchr", &[buf.at(), u64::from(b'z'), 4]), 0);
    // Bounded by the count, so a byte past it is not found.
    assert_eq!(call("memchr", &[buf.at(), u64::from(b'd'), 3]), 0);
    assert_eq!(call("memchr", &[buf.at(), u64::from(b'a'), 0]), 0);
    assert_eq!(call("memchr", &[0, u64::from(b'a'), 4]), 0);
}

// --- length and comparison ------------------------------------------------------------------

/// `strlen` counts up to the terminator and does not include it.
#[test]
fn strlen_stops_at_the_terminator() {
    let buf = Buf::text("alpha", 16);
    assert_eq!(call("strlen", &[buf.at()]), 5);

    let empty = Buf::text("", 4);
    assert_eq!(call("strlen", &[empty.at()]), 0);
    assert_eq!(call("strlen", &[0]), 0, "a null string has no length");
}

/// `strnlen` is the smaller of the real length and the limit.
///
/// The point of the function is that it does not read past the limit, so the limit winning
/// is the case that matters - not the one where the string is shorter anyway.
#[test]
fn strnlen_never_exceeds_its_limit() {
    let buf = Buf::text("alpha", 16);
    assert_eq!(call("strnlen", &[buf.at(), 3]), 3);
    assert_eq!(call("strnlen", &[buf.at(), 5]), 5);
    assert_eq!(call("strnlen", &[buf.at(), 99]), 5);
    assert_eq!(call("strnlen", &[buf.at(), 0]), 0);
}

/// `strcmp` compares the terminator too, which is what makes a prefix sort first.
///
/// Comparing only the shorter length would make `"al"` and `"alpha"` equal. Comparing the
/// terminator as well is the one extra byte that gets it right, and it is the byte an
/// optimisation drops.
#[test]
fn strcmp_compares_the_terminator_as_well() {
    let short = Buf::text("al", 16);
    let long = Buf::text("alpha", 16);
    assert!((call("strcmp", &[short.at(), long.at()]) as i64) < 0);
    assert!((call("strcmp", &[long.at(), short.at()]) as i64) > 0);

    let same = Buf::text("alpha", 16);
    assert_eq!(call("strcmp", &[long.at(), same.at()]), 0);
}

/// Two strings differing only after their terminators are equal.
///
/// The other half of the same property, and the one a length-based comparison gets right by
/// accident while a buffer-based one gets wrong.
#[test]
fn what_lies_past_a_terminator_is_not_compared() {
    let mut a = Buf::text("alpha", 16);
    let b = Buf::text("alpha", 16);
    // Scribble past the terminator of one of them.
    call("memset", &[a.at() + 6, 0xFF, 8]);
    assert_eq!(call("strcmp", &[a.at(), b.at()]), 0);
    assert_eq!(a.as_str(), "alpha");
    let _ = &mut a;
}

/// `strncmp` stops at its limit, and a limit of zero makes everything equal.
#[test]
fn strncmp_stops_at_its_limit() {
    let a = Buf::text("alphaX", 16);
    let b = Buf::text("alphaY", 16);
    assert_eq!(call("strncmp", &[a.at(), b.at(), 5]), 0);
    assert_ne!(call("strncmp", &[a.at(), b.at(), 6]), 0);
    assert_eq!(call("strncmp", &[a.at(), b.at(), 0]), 0);

    // A limit past both strings still finds them different, because the terminator is
    // reached before the limit is.
    let short = Buf::text("al", 16);
    assert_ne!(call("strncmp", &[short.at(), a.at(), 99]), 0);
}

// --- copying -------------------------------------------------------------------------------

/// `strcpy` copies the terminator, which is what makes the result a string.
#[test]
fn strcpy_copies_the_terminator_too() {
    let src = Buf::text("alpha", 16);
    let dest = Buf::zeroed(16);
    call("memset", &[dest.at(), 0xFF, 16]);
    assert_eq!(call("strcpy", &[dest.at(), src.at()]), dest.at());
    assert_eq!(&dest.bytes()[..6], b"alpha\0");
    assert_eq!(dest.bytes()[6], 0xFF, "and nothing beyond it");
}

/// `strncpy` pads the remainder with terminators.
///
/// **A partial copy left unpadded is an unterminated string**, and callers rely on the
/// padding. The standard specifies it and it is the part an obvious implementation omits.
#[test]
fn strncpy_pads_the_remainder_with_terminators() {
    let src = Buf::text("ab", 8);
    let dest = Buf::zeroed(8);
    call("memset", &[dest.at(), 0xFF, 8]);
    call("strncpy", &[dest.at(), src.at(), 5]);
    assert_eq!(&dest.bytes()[..5], b"ab\0\0\0");
    assert_eq!(dest.bytes()[5], 0xFF, "padding stops at the limit");
}

/// A source longer than the limit is truncated, and **not** terminated.
///
/// The famous sharp edge, and matching it is the job: a `strncpy` that always terminated
/// would be safer and would disagree with the guest's own expectations about the buffer.
#[test]
fn strncpy_truncates_without_terminating() {
    let src = Buf::text("alphabet", 16);
    let dest = Buf::zeroed(8);
    call("memset", &[dest.at(), 0xFF, 8]);
    call("strncpy", &[dest.at(), src.at(), 4]);
    assert_eq!(&dest.bytes()[..4], b"alph");
    assert_eq!(dest.bytes()[4], 0xFF, "no terminator was added");
}

/// A limit of zero, or a null argument, copies nothing.
#[test]
fn a_string_copy_of_nothing_copies_nothing() {
    let src = Buf::text("alpha", 8);
    let dest = Buf::zeroed(8);
    assert_eq!(call("strncpy", &[dest.at(), src.at(), 0]), dest.at());
    assert_eq!(dest.bytes(), &[0; 8]);
    assert_eq!(call("strcpy", &[dest.at(), 0]), dest.at());
    assert_eq!(call("strcpy", &[0, src.at()]), 0);
}

/// `strcat` appends at the destination's terminator.
#[test]
fn strcat_appends_at_the_existing_terminator() {
    let dest = Buf::text("alpha", 16);
    let src = Buf::text("bet", 8);
    assert_eq!(call("strcat", &[dest.at(), src.at()]), dest.at());
    assert_eq!(dest.as_str(), "alphabet");
    assert_eq!(dest.bytes()[8], 0, "and terminates the result");
}

/// `strncat`'s limit bounds the **source**, not the result.
///
/// A caller passing the size of the destination writes past the end of it. The standard
/// defines it this way, so matching the standard is the job even though the safer reading
/// is the one a reader expects.
#[test]
fn the_limit_on_strncat_bounds_the_source() {
    let dest = Buf::text("alpha", 16);
    let src = Buf::text("bethere", 16);
    call("strncat", &[dest.at(), src.at(), 3]);
    assert_eq!(dest.as_str(), "alphabet");
    assert_eq!(dest.bytes()[8], 0);

    // A limit past the source appends all of it, and still terminates.
    let more = Buf::text("!", 4);
    call("strncat", &[dest.at(), more.at(), 99]);
    assert_eq!(dest.as_str(), "alphabet!");
}

/// Appending nothing still terminates, which is what makes an empty append safe.
#[test]
fn appending_nothing_still_terminates() {
    let dest = Buf::text("alpha", 16);
    let empty = Buf::text("", 4);
    call("strncat", &[dest.at(), empty.at(), 8]);
    assert_eq!(dest.as_str(), "alpha");
    assert_eq!(dest.bytes()[5], 0);
}

// --- searching -------------------------------------------------------------------------------

/// `strchr` can find the terminator, which the standard requires.
///
/// `strchr(s, 0)` returns the **end of the string**, not null - a caller uses it to find
/// where a string ends without a second scan. Searching only up to the terminator, which is
/// the obvious loop, returns null instead.
#[test]
fn strchr_can_find_the_terminator() {
    let buf = Buf::text("alpha", 16);
    assert_eq!(call("strchr", &[buf.at(), u64::from(b'p')]), buf.at() + 2);
    assert_eq!(call("strchr", &[buf.at(), 0]), buf.at() + 5);
    assert_eq!(call("strchr", &[buf.at(), u64::from(b'z')]), 0);
    assert_eq!(call("strchr", &[0, u64::from(b'a')]), 0);
}

/// `strchr` finds the first occurrence and `strrchr` the last.
///
/// Asserted on the same input, because a pair where one is wired to the other agrees on
/// every string whose target appears once.
#[test]
fn strchr_takes_the_first_and_strrchr_the_last() {
    let buf = Buf::text("banana", 16);
    assert_eq!(call("strchr", &[buf.at(), u64::from(b'a')]), buf.at() + 1);
    assert_eq!(call("strrchr", &[buf.at(), u64::from(b'a')]), buf.at() + 5);
    assert_eq!(call("strrchr", &[buf.at(), 0]), buf.at() + 6);
    assert_eq!(call("strrchr", &[buf.at(), u64::from(b'z')]), 0);
    assert_eq!(call("strrchr", &[0, u64::from(b'a')]), 0);
}

// --- duplication ---------------------------------------------------------------------------

/// `strdup` returns memory the guest's own `free` can release.
///
/// It has to come from the same allocator, which is why this goes through the heap rather
/// than anything simpler: a caller frees what it is given, and a block `free` does not
/// recognise is either a leak or a corruption.
#[test]
fn a_duplicated_string_comes_from_the_heap_that_frees_it() {
    let src = Buf::text("alpha", 16);
    let copy = call("strdup", &[src.at()]);
    assert_ne!(copy, 0, "the allocation must succeed");
    assert_ne!(copy, src.at(), "and it must be a copy, not the original");
    assert_eq!(call("strcmp", &[copy, src.at()]), 0);
    assert_eq!(call("strlen", &[copy]), 5);

    // Writing to the copy must not touch the original.
    call("memset", &[copy, u64::from(b'z'), 5]);
    assert_eq!(src.as_str(), "alpha");

    call("free", &[copy]);
}

/// `strndup` takes at most `n` bytes and always terminates.
#[test]
fn strndup_truncates_and_always_terminates() {
    let src = Buf::text("alphabet", 16);

    let short = call("strndup", &[src.at(), 5]);
    assert_ne!(short, 0);
    assert_eq!(call("strlen", &[short]), 5, "terminated at the limit");

    let long = call("strndup", &[src.at(), 99]);
    assert_ne!(long, 0);
    assert_eq!(
        call("strlen", &[long]),
        8,
        "a limit past the string takes all of it"
    );

    call("free", &[short]);
    call("free", &[long]);
}

/// Duplicating an empty string still returns a usable, terminated block.
#[test]
fn duplicating_an_empty_string_still_returns_a_string() {
    let empty = Buf::text("", 4);
    let copy = call("strdup", &[empty.at()]);
    assert_ne!(copy, 0, "a zero-length string is still an allocation");
    assert_eq!(call("strlen", &[copy]), 0);
    call("free", &[copy]);
}

// --- tokenising ------------------------------------------------------------------------------

/// The whole `strtok` walk, in one test.
///
/// **One test on purpose.** `strtok` keeps its place in a process-wide static - which is
/// what the interface specifies and the reason `strtok_r` exists - so two tests walking at
/// once would race on it and fail for reasons neither is about. The reentrant version is
/// exercised beside it here so the two can be compared on the same input.
#[test]
fn tokenising_walks_a_string_and_writes_into_it() {
    let text = Buf::text("alpha,,beta,gamma", 32);
    let delims = Buf::text(",", 4);

    let first = call("strtok", &[text.at(), delims.at()]);
    assert_eq!(first, text.at());
    assert_eq!(
        call("strlen", &[first]),
        5,
        "the token is terminated in place"
    );

    // Consecutive delimiters are one separator, not an empty token between them.
    let second = call("strtok", &[0, delims.at()]);
    assert_eq!(call("strlen", &[second]), 4);
    let third = call("strtok", &[0, delims.at()]);
    assert_eq!(call("strlen", &[third]), 5);

    // The walk ends, and stays ended rather than restarting.
    assert_eq!(call("strtok", &[0, delims.at()]), 0);
    assert_eq!(call("strtok", &[0, delims.at()]), 0);

    // on a literal. **One terminator per token, at the delimiter that ended it** - the
    // second comma was skipped as leading separator for the next token and is still
    // sitting there, so the buffer is not the input with every delimiter replaced.
    assert_eq!(&text.bytes()[..17], b"alpha\0,beta\0gamma");

    // The reentrant walk, over its own copy, with the caller holding the place.
    let other = Buf::text("one two", 16);
    let spaces = Buf::text(" ", 4);
    let mut save: u64 = 0;
    let slot = std::ptr::from_mut(&mut save).expose_provenance() as u64;

    let a = call("strtok_r", &[other.at(), spaces.at(), slot]);
    assert_eq!(a, other.at());
    assert_ne!(save, 0, "the caller's place must be written");
    let b = call("strtok_r", &[0, spaces.at(), slot]);
    assert_eq!(call("strlen", &[b]), 3);
    assert_eq!(call("strtok_r", &[0, spaces.at(), slot]), 0);

    // A null save pointer has nowhere to resume from, and says so rather than reading one.
    assert_eq!(call("strtok_r", &[0, spaces.at(), 0]), 0);
}

/// A string of nothing but delimiters yields no tokens at all.
#[test]
fn a_string_of_only_delimiters_yields_nothing() {
    let text = Buf::text(",,,", 8);
    let delims = Buf::text(",", 4);
    let mut save: u64 = 0;
    let slot = std::ptr::from_mut(&mut save).expose_provenance() as u64;
    assert_eq!(call("strtok_r", &[text.at(), delims.at(), slot]), 0);
    assert_eq!(save, 0, "and the walk is over rather than paused");
}

/// A token running to the end of the string is still returned.
///
/// The branch where no closing delimiter is found, which is the one an implementation that
/// searches for a delimiter pair misses.
#[test]
fn a_token_running_to_the_end_is_still_a_token() {
    let text = Buf::text("tail", 8);
    let delims = Buf::text(",", 4);
    let mut save: u64 = 0;
    let slot = std::ptr::from_mut(&mut save).expose_provenance() as u64;
    assert_eq!(call("strtok_r", &[text.at(), delims.at(), slot]), text.at());
    assert_eq!(save, 0, "there is nothing to resume from");
    assert_eq!(call("strtok_r", &[0, delims.at(), slot]), 0);
}
