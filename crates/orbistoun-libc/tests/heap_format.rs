//! The allocator, formatted output, and the two functions that call back into the guest.
//!
//! # Why these three together
//!
//! They are what is left of the library once the pure string work is covered, and they
//! share the property that makes them worth testing: each one hands the guest something it
//! will immediately act on without checking. A block it will write to, a string it will
//! parse, a pointer it will dereference. A wrong answer from any of them surfaces somewhere
//! with no connection to the call that produced it, which is the failure principle 3 is
//! about.
//!
//! # Process-wide state
//!
//! Formatted writes are counted in process-wide atomics, and `getopt` and `signal` keep
//! position and handlers in statics. Tests run in parallel threads in one binary, so those
//! counters are **shared between every test here**. Assertions on them are therefore
//! written as "at least", never as an exact total, and the one test that reads them says
//! so. A test demanding an exact count would fail depending on which other test happened to
//! run first (`docs/TESTING.md`).
//!
//! What *is* asserted exactly is per-call behaviour: what a refusal writes into the
//! destination and what it answers. That is race-free, and it is the part a guest sees.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// A writable guest buffer at a real address.
struct Buf {
    storage: Vec<u8>,
    at: u64,
}

impl Buf {
    fn zeroed(size: usize) -> Self {
        Self::new(vec![0; size])
    }

    fn text(s: &str, size: usize) -> Self {
        assert!(s.len() < size, "the terminator needs room too");
        let mut v = vec![0_u8; size];
        v[..s.len()].copy_from_slice(s.as_bytes());
        Self::new(v)
    }

    /// The address comes from a mutable pointer, so writes through it are sound rather than
    /// merely working.
    fn new(mut storage: Vec<u8>) -> Self {
        let at = storage.as_mut_ptr().expose_provenance() as u64;
        Self { storage, at }
    }

    fn at(&self) -> u64 {
        self.at
    }

    fn as_str(&self) -> &str {
        let end = self
            .storage
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(self.storage.len());
        std::str::from_utf8(&self.storage[..end]).expect("test buffers hold text")
    }

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

/// Reads one byte from a guest address.
fn peek(at: u64) -> u8 {
    // SAFETY: an address this test allocated and has not freed, under the identity mapping.
    unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u8>(at as usize)) }
}

/// Writes one byte to a guest address.
fn poke(at: u64, value: u8) {
    // SAFETY: an address this test allocated and has not freed, under the identity mapping.
    unsafe {
        std::ptr::write(
            std::ptr::with_exposed_provenance_mut::<u8>(at as usize),
            value,
        );
    }
}

// --- the allocator ---------------------------------------------------------------------

/// A block from `malloc` is writable, distinct, and readable back.
///
/// The failure this rules out is the one that started the whole allocator: unimplemented,
/// `malloc` returned the placeholder error code, and the guest took *that* as its buffer
/// and handed it to `memset`, which faithfully wrote there (D128).
#[test]
fn a_malloc_block_is_real_memory_the_guest_can_use() {
    let a = call("malloc", &[64]);
    let b = call("malloc", &[64]);
    assert_ne!(a, 0, "the allocation must succeed");
    assert_ne!(b, 0);
    assert_ne!(a, b, "two live blocks must not be the same block");

    call("memset", &[a, 0xAB, 64]);
    call("memset", &[b, 0xCD, 64]);
    assert_eq!(peek(a), 0xAB);
    assert_eq!(peek(a + 63), 0xAB);
    assert_eq!(
        peek(b),
        0xCD,
        "the second block was not overwritten by the first"
    );

    call("free", &[a]);
    call("free", &[b]);
}

/// **A zero-size request answers a unique pointer, not null** (D383).
///
/// The standard permits either. This emulator's job is the platform, FreeBSD answers a
/// pointer, and the near-universal caller idiom is `if (!p) fail` - so answering null turns a
/// zero-sized request into an allocation failure. `ftpsrv` asked how many processes were
/// running, was told none, allocated nothing for the list, and reported a memory failure.
#[test]
fn allocating_nothing_answers_a_unique_pointer() {
    let empty = call("malloc", &[0]);
    assert_ne!(
        empty, 0,
        "a caller writing `if (!p) fail` must not fail here"
    );

    let another = call("malloc", &[0]);
    assert_ne!(another, empty, "and each one is its own pointer");

    assert_ne!(call("calloc", &[0, 16]), 0);
    assert_ne!(call("calloc", &[16, 0]), 0);

    // Freeable like any other, which is what makes it a real allocation rather than a token.
    call("free", &[empty]);
    call("free", &[another]);
}

/// `calloc` zeroes what it hands back, because callers rely on it.
#[test]
fn calloc_zeroes_what_it_returns() {
    let block = call("calloc", &[16, 4]);
    assert_ne!(block, 0);
    for offset in 0..64 {
        assert_eq!(peek(block + offset), 0, "byte {offset} was not zeroed");
    }
    call("free", &[block]);
}

/// `calloc` checks its own multiplication, which is half the reason it exists.
///
/// An unchecked product wraps to a small number, and the caller then writes the size it
/// asked for into a block that size never described.
#[test]
fn calloc_refuses_a_product_that_would_overflow() {
    assert_eq!(call("calloc", &[u64::MAX, 2]), 0);
    assert_eq!(call("calloc", &[1 << 62, 1 << 62]), 0);
}

/// `free` of null, and of an address this library never handed out, both decline quietly.
///
/// Declining is the point: `free` on a wild pointer must become a no-op rather than a
/// `dealloc` against a layout nobody allocated, which would corrupt a heap this process
/// shares with the emulator itself.
#[test]
fn freeing_something_that_was_never_allocated_declines() {
    assert_eq!(call("free", &[0]), 0);

    // A stack address, which carries no header this allocator wrote.
    let buf = Buf::zeroed(64);
    assert_eq!(call("free", &[buf.at() + 32]), 0);
    assert_eq!(buf.bytes(), &[0; 64], "and touched nothing");
}

/// `memalign` honours the alignment it was given.
#[test]
fn memalign_returns_an_aligned_block() {
    for align in [16_u64, 32, 64, 256, 4096] {
        let block = call("memalign", &[align, 100]);
        assert_ne!(block, 0, "alignment {align} should be satisfiable");
        assert_eq!(block % align, 0, "block {block:#x} is not {align}-aligned");
        call("free", &[block]);
    }
}

/// An alignment that is not a power of two is refused rather than rounded up.
///
/// Rounding on the caller's behalf would hide a bug in the caller, and every allocator
/// interface requires a power of two anyway.
#[test]
fn memalign_refuses_an_alignment_that_is_not_a_power_of_two() {
    assert_eq!(call("memalign", &[24, 100]), 0);
    assert_eq!(call("memalign", &[0, 100]), 0);
    assert_eq!(call("memalign", &[100, 100]), 0);
}

/// `realloc` preserves contents, and `realloc(NULL, n)` is `malloc(n)`.
#[test]
fn realloc_preserves_what_was_there() {
    let fresh = call("realloc", &[0, 32]);
    assert_ne!(fresh, 0, "realloc of null is malloc");
    call("memset", &[fresh, 0x5A, 32]);

    let grown = call("realloc", &[fresh, 256]);
    assert_ne!(grown, 0);
    for offset in 0..32 {
        assert_eq!(
            peek(grown + offset),
            0x5A,
            "byte {offset} did not survive the move"
        );
    }

    // Shrinking keeps the leading bytes and drops the rest, which is all it promises.
    poke(grown + 8, 0x11);
    let shrunk = call("realloc", &[grown, 16]);
    assert_ne!(shrunk, 0);
    assert_eq!(peek(shrunk), 0x5A);
    assert_eq!(peek(shrunk + 8), 0x11);

    call("free", &[shrunk]);
}

/// A quiet run stays quiet: no heap fill was asked for, so there is nothing to report.
///
/// The alternative - a line saying "0 allocations filled" on every run - is the noise that
/// makes a real one invisible.
#[test]
fn an_unasked_heap_fill_reports_nothing() {
    assert_eq!(orbistoun_libc::heap_fill_summary(), None);
}

// --- formatted output ----------------------------------------------------------------------

/// Renders a format through the bounded writer, returning what landed and what was claimed.
fn render(format: &str, args: &[u64]) -> (String, u64) {
    let dest = Buf::zeroed(256);
    let template = Buf::text(format, format.len() + 1);
    let mut all = vec![dest.at(), 256, template.at()];
    all.extend_from_slice(args);
    let answer = call("snprintf_s", &all);
    (dest.as_str().to_owned(), answer)
}

/// Every integer conversion renders in its own base and signedness.
///
/// `%d` and `%u` on the same bits are the pair worth asserting together: one is `-1` and
/// the other is a very large number, so a conversion wired to the wrong one is obvious
/// here and invisible on any positive input.
#[test]
fn the_integer_conversions_each_render_their_own_way() {
    let minus_one = (-1_i64) as u64;
    assert_eq!(render("%d", &[minus_one]).0, "-1");
    assert_eq!(render("%i", &[minus_one]).0, "-1");
    // `%u` is an *unsigned int*, so it reads thirty-two bits; `%lu` reads the whole word.
    assert_eq!(render("%u", &[minus_one]).0, format!("{}", u32::MAX));
    assert_eq!(render("%lu", &[minus_one]).0, format!("{}", u64::MAX));
    assert_eq!(render("%d", &[42]).0, "42");
    assert_eq!(render("%x", &[255]).0, "ff");
    assert_eq!(render("%X", &[255]).0, "FF");
    assert_eq!(render("%o", &[8]).0, "10");
    assert_eq!(render("%p", &[0x1000]).0, "0x1000");
}

/// A length modifier is consumed, and says how many of the argument's bits count.
///
/// **It used to change nothing**, on the reasoning that every argument is a register - true
/// until arguments started coming off the stack, where a slot narrower than eight bytes has
/// an unspecified upper half (D385).
#[test]
fn a_length_modifier_says_how_wide_the_argument_is() {
    for format in ["%d", "%ld", "%lld", "%zd", "%jd", "%td", "%hd", "%hhd"] {
        assert_eq!(render(format, &[42]).0, "42", "{format}");
    }
    // A value with something in every byte, so each width answers differently.
    let value = 0x1234_5678_9ABC_DEF0_u64;
    assert_eq!(render("%hhx", &[value]).0, "f0");
    assert_eq!(render("%hx", &[value]).0, "def0");
    assert_eq!(render("%x", &[value]).0, "9abcdef0");
    assert_eq!(render("%lx", &[value]).0, "123456789abcdef0");
}

/// A literal percent is emitted and consumes no argument.
///
/// The consuming half is what a test on `"%%"` alone would miss: a `%%` that took an
/// argument would shift everything after it by one.
#[test]
fn a_literal_percent_consumes_no_argument() {
    assert_eq!(render("100%%", &[]).0, "100%");
    assert_eq!(render("%d%%%d", &[1, 2]).0, "1%2");
}

/// String and character conversions read the guest's own memory.
#[test]
fn the_string_conversions_read_guest_memory() {
    let text = Buf::text("alpha", 8);
    assert_eq!(render("[%s]", &[text.at()]).0, "[alpha]");
    assert_eq!(render("%c", &[u64::from(b'Z')]).0, "Z");

    // Precision bounds a string, which is how a guest prints a field that is not
    // terminated where it wants the print to stop.
    assert_eq!(render("%.3s", &[text.at()]).0, "alp");
    assert_eq!(render("%.99s", &[text.at()]).0, "alpha");
}

/// A null string prints `(null)` rather than faulting inside formatting.
///
/// What every implementation of note does, and a guest relying on it would otherwise take
/// the emulator down from inside a diagnostic.
#[test]
fn a_null_string_argument_prints_a_placeholder() {
    assert_eq!(render("[%s]", &[0]).0, "[(null)]");
}

/// Width pads, and the flags decide with what and on which side.
#[test]
fn width_and_flags_pad_the_rendered_value() {
    assert_eq!(render("[%5d]", &[42]).0, "[   42]");
    assert_eq!(render("[%-5d]", &[42]).0, "[42   ]");
    assert_eq!(render("[%05d]", &[42]).0, "[00042]");
    // Left alignment wins over zero padding, because padding on the right with zeroes
    // would change the value being read back.
    assert_eq!(render("[%-05d]", &[42]).0, "[42   ]");
    assert_eq!(render("[%5s]", &[Buf::text("ab", 4).at()]).0, "[   ab]");
    // A width narrower than the value never truncates it.
    assert_eq!(render("[%2d]", &[12345]).0, "[12345]");
}

/// The answer is the length the whole rendering would have been, not the part that fit.
///
/// **A caller detects truncation by comparing it against the size it passed**, so reporting
/// the copied length would hide exactly the case the bounded variant exists to report.
#[test]
fn a_truncated_write_still_reports_the_full_length() {
    let dest = Buf::zeroed(8);
    let template = Buf::text("%s", 4);
    let text = Buf::text("abcdefghij", 16);

    let answer = call("snprintf_s", &[dest.at(), 8, template.at(), text.at()]);
    assert_eq!(answer, 10, "the full rendering is ten characters");
    assert_eq!(
        dest.as_str(),
        "abcdefg",
        "seven, and room for the terminator"
    );
    assert_eq!(dest.bytes()[7], 0);
}

/// A refused format writes an empty, terminated destination and answers zero.
///
/// **Not a partial rendering.** A guest receiving `"texture_"` where it expected
/// `"texture_47.gnf"` opens the wrong file, and the failure surfaces somewhere with no
/// connection to formatting. Empty is also wrong, but bounded and immediate.
#[test]
fn a_refused_format_produces_nothing_rather_than_something_plausible() {
    for format in [
        "%f",     // floating point: never arrived in an integer register at all
        "%.2f",   // and with a precision, which must not change the classification
        "%e",     //
        "%g",     //
        "%q",     // a conversion this does not implement
        "value=", // fine on its own; the trailing bare percent below is not
    ] {
        let dest = Buf::text("PREVIOUS", 32);
        let template = Buf::text(format, format.len() + 1);
        let answer = call("snprintf_s", &[dest.at(), 32, template.at(), 1]);
        if format == "value=" {
            assert_eq!(answer, 6, "a format with no conversions is not a refusal");
            continue;
        }
        assert_eq!(answer, 0, "{format} should be refused");
        assert_eq!(dest.as_str(), "", "{format} left something behind");
        assert_eq!(
            dest.bytes()[0],
            0,
            "{format} did not terminate the destination"
        );
    }
}

/// A format ending in a bare percent is malformed and is refused, not dropped.
///
/// The guest built this string and got it wrong; rendering the part before it would hand
/// back a string that looks finished.
#[test]
fn a_format_ending_in_a_bare_percent_is_refused() {
    let (text, answer) = render("done %", &[]);
    assert_eq!(answer, 0);
    assert_eq!(text, "");
}

/// A format calling for more arguments than arrived is refused rather than invented.
///
/// Six integer registers arrive and the bounded writer spends three on its own fixed
/// parameters, so the fourth conversion has nothing behind it. The rest were passed on the
/// stack, which is reachable but not from the argument array alone (D183).
#[test]
fn a_format_wanting_more_arguments_than_arrived_is_refused() {
    assert_eq!(render("%d %d %d", &[1, 2, 3]).0, "1 2 3");

    let dest = Buf::text("PREVIOUS", 32);
    let template = Buf::text("%d %d %d %d", 16);
    let answer = call("snprintf_s", &[dest.at(), 32, template.at(), 1, 2, 3]);
    assert_eq!(answer, 0, "the fourth conversion has nothing behind it");
    assert_eq!(dest.as_str(), "");
}

/// A null destination, a null format, or no room at all, each answer zero.
#[test]
fn a_write_with_nowhere_to_go_answers_zero() {
    let dest = Buf::zeroed(16);
    let template = Buf::text("%d", 4);
    assert_eq!(call("snprintf_s", &[0, 16, template.at(), 1]), 0);
    assert_eq!(call("snprintf_s", &[dest.at(), 0, template.at(), 1]), 0);
    assert_eq!(call("snprintf_s", &[dest.at(), 16, 0, 1]), 0);
    assert_eq!(
        dest.bytes()[0],
        0,
        "a null format still terminates the destination"
    );
}

/// `sprintf` renders the same way with no bound at all.
///
/// Unbounded is the whole hazard: nothing here can stop an overrun, because the guest
/// promised the buffer is large enough and there is no size to check against.
#[test]
fn sprintf_renders_without_a_bound() {
    let dest = Buf::zeroed(64);
    let template = Buf::text("%s=%d", 8);
    let name = Buf::text("width", 8);

    let answer = call("sprintf", &[dest.at(), template.at(), name.at(), 1920]);
    assert_eq!(dest.as_str(), "width=1920");
    assert_eq!(answer, 10);

    assert_eq!(call("sprintf", &[0, template.at()]), 0);
    assert_eq!(call("sprintf", &[dest.at(), 0]), 0);
}

/// Formatted writes are counted, and the first thing one could not do is remembered.
///
/// **"At least", never an exact total.** The counters are process-wide and every other test
/// in this binary contributes to them, so an exact assertion would pass or fail depending
/// on scheduling. What is checked is that a refusal moves the refused counter and that a
/// fault was recorded at all - the *kind* of the first one belongs to whichever test ran
/// first, which is not a fact about this one.
#[test]
fn formatted_writes_are_counted_and_the_first_fault_is_remembered() {
    let before = orbistoun_libc::format_stats();

    let dest = Buf::zeroed(32);
    let good = Buf::text("%d", 4);
    let bad = Buf::text("%f", 4);
    call("snprintf_s", &[dest.at(), 32, good.at(), 7]);
    call("snprintf_s", &[dest.at(), 32, bad.at(), 7]);

    let after = orbistoun_libc::format_stats();
    assert!(
        after.calls > before.calls + 1,
        "both writes should have been counted"
    );
    assert!(
        after.refused > before.refused,
        "the refusal should have been counted"
    );
    assert!(
        after.first_fault.is_some(),
        "something was refused, so a first fault must have been recorded"
    );
}

// --- calling back into the guest -------------------------------------------------------------

/// Reads a four-byte value at a guest address.
fn read_i32(at: u64) -> i32 {
    // SAFETY: an address inside a buffer this test owns, under the identity mapping.
    unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<i32>(at as usize)) }
}

/// An ordinary comparator, answering the sign of the difference.
extern "sysv64" fn ascending(a: u64, b: u64) -> u64 {
    match read_i32(a).cmp(&read_i32(b)) {
        std::cmp::Ordering::Less => (-1_i64) as u64,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// The same order, answering a **32-bit** negative with a clean upper half.
///
/// The guest answers an `int`, which occupies the low half of `rax` and leaves the rest
/// whatever it was. A whole-word test on the result reads `0x0000_0000_FFFF_FFFF` as a
/// large positive number and reverses the comparison - silently, and only for the elements
/// that happened to compare less.
extern "sysv64" fn ascending_narrow(a: u64, b: u64) -> u64 {
    match read_i32(a).cmp(&read_i32(b)) {
        std::cmp::Ordering::Less => 0x0000_0000_FFFF_FFFF,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// The address of a comparator, as a guest would pass it.
fn address_of(f: extern "sysv64" fn(u64, u64) -> u64) -> u64 {
    f as usize as u64
}

/// An array of `i32` at a real address.
fn array(values: &[i32]) -> Buf {
    let mut bytes = Vec::new();
    for v in values {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    Buf::new(bytes)
}

/// Reads an array of `i32` back out.
fn read_array(buf: &Buf, count: usize) -> Vec<i32> {
    (0..count)
        .map(|i| read_i32(buf.at() + (i * 4) as u64))
        .collect()
}

/// `qsort` sorts through a comparator in the guest's own code.
///
/// The first time this crate *makes* a call rather than answering one: the caller hands
/// over a function pointer into its own image and expects it to be used (D274).
#[test]
fn qsort_sorts_through_a_guest_comparator() {
    let values = [5_i32, 3, 9, 1, 7, 3];
    let buf = array(&values);
    call(
        "qsort",
        &[buf.at(), values.len() as u64, 4, address_of(ascending)],
    );
    assert_eq!(read_array(&buf, values.len()), [1, 3, 3, 5, 7, 9]);
}

/// A comparator answering a 32-bit negative sorts the same way as one answering a 64-bit
/// negative.
///
/// The two disagree only if the result is tested as a whole word, and then only for the
/// pairs that compare less - so a reversed answer looks like a partially sorted array
/// rather than an obviously broken one.
#[test]
fn a_thirty_two_bit_negative_result_is_still_negative() {
    let values = [5_i32, 3, 9, 1];
    let wide = array(&values);
    let narrow = array(&values);

    call("qsort", &[wide.at(), 4, 4, address_of(ascending)]);
    call("qsort", &[narrow.at(), 4, 4, address_of(ascending_narrow)]);

    assert_eq!(read_array(&wide, 4), [1, 3, 5, 9]);
    assert_eq!(read_array(&narrow, 4), read_array(&wide, 4));
}

/// Sorting fewer than two elements, or a nonsense description, changes nothing.
#[test]
fn a_sort_with_nothing_to_do_leaves_the_array_alone() {
    let values = [5_i32, 3];
    let buf = array(&values);
    for args in [
        [buf.at(), 1, 4, address_of(ascending)],
        [buf.at(), 0, 4, address_of(ascending)],
        [buf.at(), 2, 0, address_of(ascending)],
        [buf.at(), 2, 4, 0],
        [0, 2, 4, address_of(ascending)],
    ] {
        call("qsort", &args);
        assert_eq!(read_array(&buf, 2), [5, 3], "the array was disturbed");
    }
}

/// `bsearch` answers a pointer into the array, and null for a miss.
///
/// **Null, never an error code.** A count-shaped placeholder here would be a wild pointer
/// the guest dereferences immediately, which is the shape this project keeps finding
/// (D125, D273).
#[test]
fn bsearch_answers_a_pointer_into_the_array_or_null() {
    let values = [1_i32, 3, 5, 7, 9, 11];
    let buf = array(&values);
    let compare = address_of(ascending);

    for (index, wanted) in values.iter().enumerate() {
        let key = array(&[*wanted]);
        let found = call(
            "bsearch",
            &[key.at(), buf.at(), values.len() as u64, 4, compare],
        );
        assert_eq!(
            found,
            buf.at() + (index * 4) as u64,
            "{wanted} should be found at index {index}"
        );
    }

    for missing in [0_i32, 4, 12] {
        let key = array(&[missing]);
        assert_eq!(
            call(
                "bsearch",
                &[key.at(), buf.at(), values.len() as u64, 4, compare]
            ),
            0,
            "{missing} is not in the array"
        );
    }
}

/// A search of nothing finds nothing, and never reads the array.
#[test]
fn a_search_with_nothing_to_search_finds_nothing() {
    let buf = array(&[1_i32, 2]);
    let key = array(&[1_i32]);
    let compare = address_of(ascending);
    for args in [
        [key.at(), buf.at(), 0, 4, compare],
        [key.at(), buf.at(), 2, 0, compare],
        [key.at(), buf.at(), 2, 4, 0],
        [key.at(), 0, 2, 4, compare],
        [0, buf.at(), 2, 4, compare],
    ] {
        assert_eq!(call("bsearch", &args), 0);
    }
}

// --- the rest -----------------------------------------------------------------------------

/// `errno` is a real, writable, thread-local address.
///
/// **It must be real because every use of `errno` in a guest dereferences it.** A stub
/// answering a placeholder hands back a wild pointer the guest then reads and writes, which
/// is exactly what happened: a read of orbistoun's own placeholder value, taken as an
/// address (D344).
#[test]
fn errno_is_a_real_address_and_is_not_shared_between_threads() {
    let mine = call("__error", &[]);
    assert_ne!(mine, 0, "errno must have storage, not a value");
    assert_eq!(
        call("__error", &[]),
        mine,
        "and it must be stable per thread"
    );

    // Writable, which is the half a read-only answer would fail.
    poke(mine, 9);
    assert_eq!(peek(mine), 9);

    let theirs = std::thread::spawn(|| call("__error", &[]))
        .join()
        .expect("the thread runs");
    assert_ne!(
        theirs, mine,
        "one guest thread's failure must not appear as another's"
    );
}

/// `signal` records a handler and answers the one it replaced.
///
/// **One test, because the handler table is process-wide** - two tests installing on the
/// same number would each see the other's handler. Distinct numbers within one test keep
/// the sequence deterministic.
///
/// What must never happen is a failure answer: the first thing a network server does is ask
/// for `SIGPIPE` to be ignored, and an error there sends a correctly written program down
/// its error path before it has done anything at all (D343).
#[test]
fn signal_records_a_handler_and_reports_the_previous_one() {
    const SIGPIPE: u64 = 0xd;

    assert_eq!(
        call("signal", &[SIGPIPE, 1]),
        0,
        "nothing was installed, and zero is how that is spelled"
    );
    assert_eq!(
        call("signal", &[SIGPIPE, 2]),
        1,
        "the handler it replaced comes back"
    );
    assert_eq!(call("signal", &[SIGPIPE, 0]), 2);

    // A number past the table answers the default rather than refusing - refusing would be
    // a claim about which signals exist that nothing here has measured.
    assert_eq!(call("signal", &[9999, 1]), 0);
    assert_eq!(call("signal", &[u64::MAX, 1]), 0);
}

/// `getopt` answers "nothing left" as a 32-bit `-1`.
///
/// The guest reads `eax`, so what it must find there is `-1` in 32 bits. Answering a 64-bit
/// `-1` would look identical in a debugger and be a different number to the guest.
#[test]
fn getopt_reports_nothing_left_as_a_thirty_two_bit_minus_one() {
    let options = Buf::text("abc", 8);
    let argv = Buf::zeroed(16);

    let done = 0xFFFF_FFFF_u64;
    assert_eq!(call("getopt", &[0, argv.at(), options.at()]), done);
    assert_eq!(call("getopt", &[1, argv.at(), options.at()]), done);

    // A count that did not come from a real process image is refused rather than walked:
    // iterating a pointer array sized by a stray value would fault inside this call, and be
    // reported as the guest's fault.
    assert_eq!(call("getopt", &[u64::MAX, argv.at(), options.at()]), done);
    assert_eq!(call("getopt", &[1_000_000, argv.at(), options.at()]), done);
    assert_eq!(call("getopt", &[2, 0, options.at()]), done);
    assert_eq!(call("getopt", &[2, argv.at(), 0]), done);
}

/// Registration functions accept, and answer that they accepted.
///
/// A non-zero answer from either makes a C++ runtime believe registration failed, and the
/// standard behaviour then is to abort - so "accepted and never run" is the honest option
/// and "refused" is not (D124).
#[test]
fn the_registration_functions_accept_rather_than_refuse() {
    assert_eq!(call("atexit", &[0x1000]), 0);
    assert_eq!(call("__cxa_atexit", &[0x1000, 0x2000, 0x3000]), 0);
}
