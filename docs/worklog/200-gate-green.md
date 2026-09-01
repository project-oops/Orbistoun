# 2026-08-29 - Gate green


First full green gate of the session. Two failures to clear, and one was mine.

**Mine:** `ORBISTOUN_BSS_FILL` was declared twice in `orbistoun-env`'s list. The earlier
dedupe only caught *adjacent* duplicates, and these were not adjacent - so the const was
removed and two list entries survived. `every_name_is_unique_and_carries_the_prefix` caught
it, which is exactly what that test is for.

**Not mine, fixed anyway:** five files carrying line-continued string literals, left by the
parallel session. With the tree quiet they were converted here - a gate that has been red
for a day stops being read.

### `concat!` is not a free substitution

Converting them broke the build three times with *there is no argument named `says`*.
An implicit format capture needs a **literal**, and `concat!` is a macro call - so
`format!("{says}")` works and `format!(concat!("{says}"))` does not (D362). Named arguments
keep both.

Worth recording because the gate gives the advice and cannot know about the exception: it
sees a backslash and says use `concat!`, which is right every time but this one.

### A stale binary, twice (D363)

The gate's five `orbistoun-gen` render failures were not a defect: `repo()` derives the
workspace root from `CARGO_MANIFEST_DIR`, baked in at compile time, so a test binary built
before the move to `OOPS/` carried a path that no longer exists. Rebuild, five pass.

And it was a **class**, not an incident: clearing five in `orbistoun-gen` produced seven
more elsewhere. Six files across the workspace bake `CARGO_MANIFEST_DIR`, all invalidated by
the move, none of them stale by cargo's reckoning because no source changed.
`grep -rl CARGO_MANIFEST_DIR` finds them and `touch` fixes them - the remedy after any move
of this repository.

Second shape today - the first was `include_str!` holding a knowledge file that had
already been fixed on disk. Both name the *file* in the error while the *binary* is the
stale thing, and both bite hardest right after something is known to have changed, which is
what makes the failure read as impossible.


