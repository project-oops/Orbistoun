# Fixed-address tests contend across crates *(wrong, then fixed, now closed)*


**The diagnosis was wrong and cost four gate runs.** It was never about addresses, and
`orbistoun-mem` was never involved.

Two tests in `orbistoun-abi` share one `static SEEN: [AtomicU64; 6]` - the array the
emitted machine code writes its arguments into - and `cargo` runs them on parallel
threads within one binary. Each could see the other's arguments.

The failure said so all along: *"argument 0 (rdi) arrived wrong - left: 3"*. Three is
not a corrupted address. It is `the_call_can_be_made_repeatedly` on its third round,
which writes exactly 3 into that slot. Reading the value rather than the shape of the
failure would have found this the first time.

Fixed by serialising the two tests on a mutex. Eleven passes, five runs running.

**The lesson worth keeping**: "intermittent under a full workspace run" was assumed to
mean cross-crate contention, and that assumption survived three sightings because it was
plausible and nobody checked it against the number in the message. An entry recording a
wrong cause is worse than one recording an unknown cause - it stops anybody looking.

**The half of this inside one crate is now fixed, and it bit exactly as predicted.** A
test added to `orbistoun-mem` picked `TEST_BASE + 0x300_0000` by reading the file and
choosing a gap - and a test three functions further down was already using it. Tests in a
binary run on parallel threads, so the two raced and one failed about one run in ten:
often enough to be real, rare enough to be dismissed. `stack.rs` hands out bases from a
counter now, so no test picks an address at all.

**The cross-crate half is now closed** (D324). `orbistoun-mem::test_bases` holds one range
per crate and hands out addresses inside it, so nobody picks a number at either level -
not a test within a binary, and not a crate within the workspace. A test asserts the ranges
are distinct, because the table is exactly the sort of thing somebody appends to in a hurry.

It was closed because the hazard bit a third time, in the way the entry above predicts
almost word for word: four new tests in `orbistoun-thunk` took the shipped base and raced,
and the first repair **picked offsets by hand** - which is the documented way to
reintroduce it.

