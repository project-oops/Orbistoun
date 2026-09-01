# 2026-08-27 - Data imports finally get something they can dereference


D307's deferred half, done (D323). `DataBlocks` reserves one zeroed page per import that
names data, in its own address range, and `ImportResolver` asks it before the thunk table.
An import naming code misses and falls through to its stub, byte for byte as before - which
is the property that stops this touching every guest at once, and it is asserted rather
than assumed.

Proven by test: distinct storage per import, reads as zero, accepts a write, resolver prefers
data, functions unaffected.

**Not proven: that it moves any guest.** `PPSA02664` went from `image+0xafc959` to
`image+0xafcc08` with two more imports reached - but a learned policy entry changed in the
same interval and the report said so itself. The `FURTHER` is unattributable and is not
claimed. This is a fix justified by what it stops being wrong, not by a wall it moved.

### Four self-inflicted failures in one unit of work

Worth listing, because they are all the same shape - the tooling around the change being
wrong rather than the change:

1. **A flaky test I wrote.** Four tests reserving the shipped base, run in parallel in one
   process; whichever arrived second got `Conflict`. Each has its own base now. It would
   have surfaced intermittently in CI rather than immediately here.
2. **`cargo fmt` orphaned a `SAFETY` comment** by collapsing an `assert_eq!` around an
   `unsafe` block. Bind the value to a name first; the lint is `deny` and caught it.
3. **A stolen doc comment**, third time this session: inserting a new method above a
   documented one leaves the documentation attached to the newcomer. Check what is directly
   above the insertion point, every time.
4. **A line limit**, which was the right complaint - the import-resolution step is its own
   decision and is now its own function.

### And a note on the workspace

`./orbistoun.sh check` cannot pass right now for reasons that are not this work: a
concurrent session has `orbistoun-submit` mid-write (a missing `use`) and has changed
`Preferences::load` to take three arguments without updating `app.rs`. Verified this
session's crates directly instead - clippy clean, all tests passing. The full gate is owed
once the workspace compiles again.

