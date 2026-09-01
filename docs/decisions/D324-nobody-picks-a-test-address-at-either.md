# D324 - Nobody picks a test address, at either level


**decided** · 2026-08-27 · closes the cross-crate half of a `docs/BACKLOG.md` entry

Anything built on `orbistoun-mem` reserves **real host memory at fixed addresses** in its
tests, so two tests using the same base race and one fails. It has now happened three times,
and every time the address was chosen by a person reading a file and picking a gap:

1. In `orbistoun-mem`, a new test took `+0x300_0000` and a test three functions later was
   already on it. One run in ten - often enough to be real, rare enough to be dismissed.
2. In `orbistoun-thunk`, four new tests all took the shipped base and whichever arrived
   second got `Conflict` (D323).
3. **In the repair for (2)**, which picked offsets by hand - the exact move the backlog
   entry describes as the way to reintroduce the bug, made while reading that entry's
   neighbourhood.

`stack.rs` had already removed the choice *within* a binary with a counter, and the backlog
recorded that the same hazard **between** binaries was still open: `cargo test` runs several
at once and a per-binary counter cannot see another binary.

`orbistoun-mem::test_bases` closes it. A crate takes a [`Range`], a test takes the next
address from it, and the crate numbers live in one table so the property that matters -
**they are distinct** - is asserted in a test rather than trusted to review.

### Why it ships rather than hiding behind `cfg(test)`

`#[cfg(test)]` items are invisible to dependents, so every crate would define its own - and
two of them choosing the same range is the failure being fixed, one level up. It is three
constants and a counter, and a crate that ignores it pays nothing.

### The part worth keeping

Three occurrences, and the diagnosis was wrong the first time in a way that cost four gate
runs - "intermittent under a full workspace run" was assumed to mean contention and nobody
checked the number in the failure message. **A convention documented in prose is one that
gets read after the third occurrence.** Handing out the addresses removes the choice, and a
removed choice cannot be made wrongly in a hurry.

