# D209 - The table generators are a crate, behind a seam that replays a recording


**decided** · 2026-08-24

`crates/orbistoun-gen` solves the shader data tables under `crates/orbistoun-shader/data/`
by assembling probes and reading the bytes back, rather than transcribing them from a
document (D085).

### The constraint that shapes it

**The reference assembler is not available where this is worked on.** The solvers need
`llvm-mc` built with the AMDGPU target; the LLVM most hosts have does not include it, and on
Windows `llvm-mc` is not shipped at all. `tools/toolchain/setup.sh` builds a VM that has it.

A generator only runnable on one machine has a worse consequence than inconvenience:
**nothing can check that a committed table still matches what produces it.** A hand-edited
row would survive indefinitely, and the tables are exactly the kind of thing somebody edits
by hand when a value looks wrong.

### The seam

`assembler::Source` is either a live `llvm-mc` or **a replay of a committed recording of
one**. Replay needs nothing installed, so `./orbistoun.sh tables` regenerates and diffs on
any machine, in `check`, and fails on a hand-edited table. Tested by making it fire.

Only the two generators whose output is committed are covered. `encodings` writes no file to
diff, and its sweeps are 9.5 MB against 772 KB for the other two - one file of which exceeds
the provenance guard's own 1 MB limit.

**A recording carries its own input and replaying checks it.** A solver whose probe list has
changed would otherwise be handed the old answers to new questions, which surfaces as a wrong
table rather than as a stale recording.

**Keys are derived from the probe text, not from call order.** This is not a nicety, and it
was found by the diff rather than by reasoning: `derive_symbolic_codes` makes forty-seven
probes, and under a single shared key replay handed all of them one canned answer. The
symbolic codes came out empty and two whole families - `EXP` and `VINTRP` - dropped out of the
operand table. Live it is invisible, because each call re-invokes the assembler. It is wrong
only on replay, which is the mode that has to be trusted.

Recording is a separate act (`--record`) rather than a cache. A cache decides for itself when
it is stale; a committed recording is a decision somebody made.

### A run that produces nothing must fail

`fixtures` and `operands` refuse rather than writing an empty table. Without a toolchain
every source is skipped - and the skip path has already deleted each `.txt` on the way past.
Without the guard the run then writes a `mnemonics.toml` with no mnemonics in it, prints
"0 fixtures", and exits zero: the reference output the differential suite exists to compare
against is gone, and nothing has said so. On any machine without an AMDGPU-enabled LLVM,
which is most of them.

### Where the difficulty is, and where the tests are

The subprocess call is a dozen lines; the bit arithmetic is two thousand. So `solve`,
`table`, `patterns` and `operands` are pure functions over numbers, unit-tested with no
toolchain - including four cases that produce a **wrong table rather than an error**: a
rejected probe shifting the input pairing of every later sample; a partial trailing word
padded rather than dropped; an operand accepted with *different bits* recorded as implicit;
and an implicit slot at word 0 bit 0 colliding with a real field there.

`tests/rendering.rs` covers the other half - it reads each committed table, hands the rows
back to the renderer, and requires the result to match the file it came from. Formatting is
where a generator silently differs, and hundreds of rows of repeated structure is exactly
where one wrong separator still looks plausible in a diff.

### `encodings` reports rather than writes

`data/encodings.toml` is not purely generated: it carries the reasoning behind each row and
citations into the published reference, which is where a wrong row gets *corrected* from. A
person edits it, acting on what the solver says. Overwriting it would throw the prose away
and leave a table nobody could check without the document that produced it.

