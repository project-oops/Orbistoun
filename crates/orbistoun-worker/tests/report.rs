//! The pieces a fault report is assembled from.
//!
//! # Why this is worth more than its size suggests
//!
//! `Line` runs inside a fault handler, where the allocator lock may be held by the code that
//! just crashed - so it allocates nothing, formats its own hexadecimal, and truncates rather
//! than growing. None of that can be stepped through: by the time it runs, the process is
//! already in the state that made it run. **A bug here appears as a garbled message at the
//! exact moment the message is most needed**, and principle 3 says a report is as capable of
//! plausible output as a stub is.
//!
//! The truncation is the clearest case. It is a guard, and a guard nobody has watched reject
//! something is a guard nobody knows anything about - so it is made to fail here, at a
//! boundary, rather than trusted.
//!
//! # Process-wide state
//!
//! Regions and import labels are global and set once, so everything depending on them is in a
//! single test. `Line` itself depends on the regions only through `address`, and the tests
//! that do not use `address` are free-standing.

use orbistoun_worker::report::{Line, Region, describe_region, label_of, locate, name_imports};

/// Renders what a line holds, for comparison.
fn rendered(line: &Line) -> String {
    String::from_utf8(line.as_bytes().to_vec()).expect("these lines are ASCII")
}

// --- the line builder ----------------------------------------------------------------------

/// A fresh line holds nothing, however it was made.
#[test]
fn a_fresh_line_is_empty() {
    assert_eq!(Line::new().as_bytes(), b"");
    assert_eq!(Line::default().as_bytes(), b"");
}

/// Text is appended in order and nothing is inserted between pieces.
///
/// The report's wording depends on it: `kind` carries its own preposition, so anything
/// helpfully adding a separator here would put one in the middle of "read of".
#[test]
fn text_is_appended_exactly_as_given() {
    let mut line = Line::new();
    line.text("read of ").text("something");
    assert_eq!(rendered(&line), "read of something");
}

/// Hexadecimal is rendered without a leading zero run, and always with the prefix.
///
/// Hand-rolled because the formatting machinery allocates, which is the whole reason this
/// type exists - and hand-rolled digit emission is where an off-by-one lives. Zero is the
/// case a loop that tests before it writes gets wrong, producing `0x` with no digits at all.
#[test]
fn hexadecimal_is_rendered_without_a_leading_zero_run() {
    for (value, want) in [
        (0_u64, "0x0"),
        (1, "0x1"),
        (9, "0x9"),
        (0xA, "0xa"),
        (0xF, "0xf"),
        (0x10, "0x10"),
        (0xDEAD_BEEF, "0xdeadbeef"),
        (0x7FFF_0002, "0x7fff0002"),
        (u64::MAX, "0xffffffffffffffff"),
    ] {
        let mut line = Line::new();
        line.hex(value);
        assert_eq!(rendered(&line), want, "hex({value:#x})");
    }
}

/// Digits come out most significant first.
///
/// A hand-rolled conversion produces them in the opposite order and has to reverse them, so
/// a value whose digits are not a palindrome is the only one that catches a missed reversal.
/// `0x1234` does; `0xEEEE` would not.
#[test]
fn hexadecimal_digits_are_not_reversed() {
    let mut line = Line::new();
    line.hex(0x1234_5678_9abc_def0);
    assert_eq!(rendered(&line), "0x123456789abcdef0");
}

/// Appends chain, so a report reads as one statement.
#[test]
fn appends_chain_in_the_order_they_are_written() {
    let mut line = Line::new();
    line.text("illegal instruction at ")
        .hex(0x4000)
        .text(", rsp ")
        .hex(0x7FF0);
    assert_eq!(rendered(&line), "illegal instruction at 0x4000, rsp 0x7ff0");
}

/// Text past the end is dropped, not wrapped, and never overflows.
///
/// **Made to fail here rather than trusted.** The buffer is fixed and the handler cannot
/// grow it, so the only question is which of the two wrong things happens at the boundary:
/// losing the tail, or writing past the end of a fixed array in a process that is already
/// crashing.
#[test]
fn text_past_the_end_is_dropped_rather_than_wrapped() {
    let mut line = Line::new();
    let filler = "x".repeat(Line::CAPACITY);
    line.text(&filler);
    assert_eq!(line.as_bytes().len(), Line::CAPACITY, "exactly full");

    line.text("this cannot fit");
    assert_eq!(
        line.as_bytes().len(),
        Line::CAPACITY,
        "and stays exactly full rather than growing or wrapping"
    );
    assert!(
        line.as_bytes().iter().all(|b| *b == b'x'),
        "nothing wrapped around to the start"
    );
}

/// A line filled to one byte short still refuses to overrun.
///
/// The off-by-one boundary: there is room for one more byte and the text is longer, so the
/// partial append is the case a `>=` in the wrong place turns into a panic.
#[test]
fn a_line_one_byte_short_of_full_takes_only_what_fits() {
    let mut line = Line::new();
    line.text(&"y".repeat(Line::CAPACITY - 1));
    line.text("AB");
    assert_eq!(line.as_bytes().len(), Line::CAPACITY);
    assert_eq!(
        line.as_bytes()[Line::CAPACITY - 1],
        b'A',
        "the first byte that fits is taken, and the rest dropped"
    );
}

/// A number that will not fit is truncated without overrunning either.
///
/// The digit loop has its own bounds check, separate from the one in `text` - so a full line
/// asked for a long number exercises a guard nothing else reaches.
#[test]
fn a_number_that_does_not_fit_is_truncated_too() {
    let mut line = Line::new();
    line.text(&"z".repeat(Line::CAPACITY - 3));
    line.hex(u64::MAX);
    assert_eq!(
        line.as_bytes().len(),
        Line::CAPACITY,
        "the prefix and one digit fit; the rest is dropped"
    );
    assert_eq!(&line.as_bytes()[Line::CAPACITY - 3..], b"0xf");
}

// --- regions -----------------------------------------------------------------------------

/// Every region and label question, in one test.
///
/// **One test on purpose.** The region table and the import labels are process-wide - the
/// handler cannot chase a pointer the faulting code may have invalidated, so they are fixed
/// statics - and a second test registering its own regions would be answering questions about
/// this one's.
#[test]
fn an_address_is_named_by_the_region_that_actually_contains_it() {
    const IMAGE: u64 = 0x0040_0000;
    const STUBS: u64 = 0x0050_0000;
    const STACK: u64 = 0x7000_0000;
    const LEN: u64 = 0x1000;

    // Before anything is registered, nothing can be named - and saying so is the honest
    // answer rather than guessing at the nearest region.
    assert_eq!(locate(IMAGE), None, "nothing is registered yet");

    describe_region(Region::Image, IMAGE, LEN);
    describe_region(Region::Stubs, STUBS, LEN);
    describe_region(Region::Stack, STACK, LEN);

    assert_eq!(
        locate(IMAGE),
        Some(("image", 0)),
        "the first byte is inside"
    );
    assert_eq!(
        locate(IMAGE + LEN - 1),
        Some(("image", LEN - 1)),
        "and so is the last"
    );
    assert_eq!(
        locate(IMAGE + LEN),
        None,
        "one past the end is outside, which is the boundary a <= would get wrong"
    );
    assert_eq!(locate(IMAGE - 1), None, "and so is one below the base");

    // Each region is named for itself, not for whichever was registered first.
    assert_eq!(locate(STUBS + 0x10), Some(("stubs", 0x10)));
    assert_eq!(locate(STACK + 0x20), Some(("stack", 0x20)));

    // An address in no region is a number, and stays one.
    assert_eq!(locate(0x1234), None);
    assert_eq!(locate(0), None);
    assert_eq!(
        locate(u64::MAX),
        None,
        "and the top of the space does not wrap"
    );

    // --- what a line does with them ------------------------------------------------------

    let mut known = Line::new();
    known.text("read of ").address(IMAGE + 0x24);
    assert_eq!(
        rendered(&known),
        "read of 0x400024 (image+0x24)",
        "an address inside a region carries where it is"
    );

    let mut unknown = Line::new();
    unknown.address(0x1234);
    assert_eq!(
        rendered(&unknown),
        "0x1234",
        concat!(
            "and one outside every region is just the number - naming it anyway would be an ",
            "invention at the moment a reader most needs the truth"
        )
    );

    // --- import labels -------------------------------------------------------------------

    assert_eq!(
        label_of(0),
        None,
        "with nothing recorded, a bare index is the answer rather than an invented name"
    );

    name_imports(vec![
        "libkernel::sceKernelAllocateDirectMemory".to_owned(),
        String::new(),
        "libc::memcpy".to_owned(),
    ]);

    assert_eq!(
        label_of(0),
        Some("libkernel::sceKernelAllocateDirectMemory")
    );
    assert_eq!(
        label_of(1),
        None,
        "an empty entry means the symbol is not an import, which is most of the table"
    );
    assert_eq!(label_of(2), Some("libc::memcpy"));
    assert_eq!(
        label_of(9999),
        None,
        "an index past the table is unknown rather than a panic in a fault handler"
    );
}
