# The split opcode, and a test that asked to be deleted


MTBUF's opcode is three bits at 18:16 of the first word and a fourth at bit 53 - bit 21 of
the second. The table read only the contiguous part, so every half-precision variant
decoded as the operation it is a variant of: `tbuffer_load_format_d16_x` reported as
`tbuffer_load_format_x`, differing in the one bit nobody looked at.

`Encoding` now carries an optional `opcode_extension`. One continuation rather than a list
of pieces, because that covers every split this instruction set actually has.

Also corrected D105's stated reason for leaving it open. It said MTBUF was "refused in its
entirety pending a resource model" - true when written, stale since MUBUF landed. The
resource model exists; a typed buffer access is the same descriptor and the same addressing
with a format conversion on top.

### Surprises

**The pinned-gap test worked exactly as designed, which was still a small surprise to
watch.** `the_typed_buffer_variants_are_known_to_be_conflated` was written as a *passing*
test asserting the conflation existed, so that closing the gap would fail here and name the
notes needing updated. It failed with that message. Kept inverted, and strengthened: it
asserts the variant is its counterpart **plus eight** rather than merely different, because
a continuation shifted to the wrong place also produces two distinct numbers.

**The rule lives in three places and only one of them is Rust.** `classify()` is
reimplemented in the generators. Updating the decoder alone would have left them
classifying a half-precision variant as its counterpart and emitting a second name for an
opcode that already had one - caught by the name table's duplicate refusal, but only by
luck of that guard existing. Changed together.

**A blind text replacement ate `class Sample`.** Replacing `classify` by cutting from
`def classify(` to the next column-zero `def ` swallowed everything between, and in one of
the two files that was a class definition and a regex. It failed loudly at the next run,
and the staged copy restored it byte-identically - but the lesson is that "next top-level
def" is not a function boundary, and I had no reason to think it was.

**The workspace does not currently build, and not because of this.** `orbistoun-elf` has a
missing import in a test module - another thread's file, mid-edit. My crates were checked
directly instead: 256 tests, clean clippy.

