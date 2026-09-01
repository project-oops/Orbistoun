# D363 - A compile-time constant fails as though the source were broken


**decided** · 2026-08-29

Twice in one day, and the second time cost a diagnosis because the first had not been
written down.

**`include_str!` on a knowledge file.** A parallel session left `libkernel.toml` with a
duplicate key; the emulator panicked at startup naming that file. They fixed it within the
minute and it **kept panicking** - because the knowledge files are embedded at compile time,
and the binary still held the broken copy. `tomllib` said the file parsed. Nothing in the
error mentioned the binary.

**`CARGO_MANIFEST_DIR` after the move to `OOPS/`.** Five `orbistoun-gen` render tests failed
with *"The system cannot find the path specified"* against tables that were plainly there.
`repo()` derives the workspace root from `CARGO_MANIFEST_DIR`, which is baked in when the
test is compiled - so a binary built before the move carried the old path, a path
that no longer exists. `touch` and rebuild: five passed.

### The shape

**A file that is read at build time fails against its old contents, and the message names
the file.** Every instinct then goes to the file, which is correct and current, and the
minutes go by. The binary is the stale thing and nothing says so.

It bites hardest exactly when it is least expected: after somebody else fixes something, and
after a directory move - both moments when the source is *known* to have just changed, which
is precisely why the failure reads as impossible.

Worth a line in a session's head: **if a data file or a path constant fails and the file
looks right, rebuild before investigating.**

### It was a class, not two incidents

Chasing the failures one gate at a time was the wrong shape: clearing five in
`orbistoun-gen` produced seven more elsewhere. **Every test binary that bakes
`CARGO_MANIFEST_DIR` was invalidated by the move**, and cargo cannot know - no source
changed, so nothing is stale by its reckoning.

Six files across the workspace do it:

```
orbistoun-gen/tests/rendering.rs        orbistoun-probe/tests/conformance.rs
orbistoun-gpu/tests/vocabulary.rs       orbistoun-shader/tests/differential.rs
orbistoun-overrides/tests/frontier.rs   orbistoun-translate/tests/execute.rs
```

`grep -rl CARGO_MANIFEST_DIR` finds them, `touch` forces the rebuild, and all six pass. That
is the remedy after any move of this repository, and it is worth knowing in one go rather
than discovering a gate at a time.

### Not worth engineering around

An `include_str!` is right for the knowledge files - a portable single binary carries them
with nothing to lose - and `CARGO_MANIFEST_DIR` is right for finding the workspace from a
test. Both would be worse as runtime lookups. The cost is a rebuild after a move, which is
cheap once it is a known cost rather than a mystery.


