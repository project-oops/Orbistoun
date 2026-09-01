# orbistoun-gen

Offline generators for the shader data tables. **Not part of the emulator.**

**Models:** the reference-assembler interface, and the solvers that turn its output into the
`.toml` tables `orbistoun-shader` reads.

**Deliberately fakes:** nothing. A code the assembler will not accept is reported as
refused, never guessed at.

## What it produces

| Command | Writes |
|---|---|
| `target <field>` | nothing - prints one constant, so a shell script reads the same source the solvers do |
| `buffer-formats` | `crates/orbistoun-shader/data/buffer-formats.toml` |
| `operands` | `crates/orbistoun-shader/data/opcode-operands.toml` |
| `fixtures` | `tests/fixtures/*` and `crates/orbistoun-shader/data/mnemonics.toml` |
| `encodings` | nothing - reports what it solved |

`encodings` reports rather than writes because `crates/orbistoun-shader/data/encodings.toml` is not purely
generated: it carries the reasoning behind each row and citations into the published
reference, which is where a wrong row gets *corrected* from. A person edits it, acting on
what the solver says.

## Running it needs a toolchain. Checking it does not.

The solvers get their bytes from `llvm-mc` with the AMDGPU target, which most machines -
including CI - do not have. `tools/toolchain/setup.sh` builds a VM that does.

**Everything else works without one**, because the assembler call is a seam:

```bash
# Anywhere, no toolchain: replay committed recordings and diff against the tables.
./bin/orbistoun tables

# In the VM, after changing a probe file or retargeting: re-record.
sh tools/toolchain/run.sh env CARGO_TARGET_DIR=/tmp/orb-target \
    cargo run --release -p orbistoun-gen -- --record crates/orbistoun-gen/tests/fixtures/transcripts operands
```

A recording carries its own input, and replaying checks it. A solver whose probe list has
changed since the recording was taken would otherwise be handed the old answers to new
questions - and that shows up as a wrong table rather than as a stale recording.

Keys are derived from the probe text, not from call order, so a recording matches by *what
was asked*. That is not a nicety: the symbolic-code probes make forty-seven calls, and under
a single shared key replay hands all of them the same canned answer - which silently drops
two whole families out of the operand table.

Recording is a separate act rather than a cache. A cache decides for itself when it is
stale; a committed recording is a decision somebody made.

## What is checked, and where

| check | needs a toolchain | catches |
|---|---|---|
| `./bin/orbistoun tables` | no | a table edited by hand, or a solver that changed what it produces |
| `tests/rendering.rs` | no | a formatting change in the renderer; a `.gcn` disagreeing with its `.txt` |
| unit tests in `solve`, `table`, `patterns`, `operands` | no | the bit arithmetic, including four cases that produce a *wrong table* rather than an error |
| a live run in the VM | yes | everything above, against the real reference |

The four solver cases worth naming, because each fails silently rather than loudly: a
rejected probe shifting the input pairing of every later sample; a partial trailing word
padded rather than dropped; an operand accepted with *different bits* recorded as implicit;
and an implicit slot at word 0 bit 0 colliding with a real field there.

**Status:** complete and verified against a live reference assembler. tests, plus the
replay diff in `check`.
