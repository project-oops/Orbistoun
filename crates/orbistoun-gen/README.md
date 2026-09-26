# orbistoun-gen

Offline generators for the shader data tables. It is not part of the emulator.

It holds the reference-assembler interface and the solvers that turn the assembler's output
into the `.toml` tables [orbistoun-shader](../orbistoun-shader/) reads. A code the assembler
does not accept is reported as refused, never guessed at.

## Commands

| Command | Writes |
|---|---|
| `target <field>` | nothing - prints one constant, so a shell script reads the same source the solvers do |
| `buffer-formats` | `crates/orbistoun-shader/data/buffer-formats.toml` |
| `operands` | `crates/orbistoun-shader/data/opcode-operands.toml` |
| `fixtures` | `tests/fixtures/*` and `crates/orbistoun-shader/data/mnemonics.toml` |
| `encodings` | nothing - reports what it solved |

`encodings` reports rather than writes because `crates/orbistoun-shader/data/encodings.toml`
is not purely generated: it carries the reasoning behind each row and citations into the
published reference, which is where a wrong row is corrected from. A person edits it, acting
on what the solver says.

## Toolchain and replay

The solvers get their bytes from `llvm-mc` with the AMDGPU target, which most machines,
including CI, do not have. `tools/toolchain/setup.sh` builds a VM that does.

Checking needs no toolchain, because the assembler call is a seam:

```bash
# Anywhere, no toolchain: replay committed recordings and diff against the tables.
./bin/orbistoun tables

# In the VM, after changing a probe file or retargeting: re-record.
sh tools/toolchain/run.sh env CARGO_TARGET_DIR=/tmp/orb-target \
    cargo run --release -p orbistoun-gen -- --record crates/orbistoun-gen/tests/fixtures/transcripts operands
```

Where the VM cannot mount the repository (Multipass's default on Windows, where
`local.privileged-mounts` is off and turning it on is a privileged machine-wide setting),
copy the tree in and the generated files back:

```bash
sh tools/toolchain/sync.sh push
sh tools/toolchain/run.sh env CARGO_TARGET_DIR=/tmp/orb-target \
    cargo run --release -p orbistoun-gen -- --record crates/orbistoun-gen/tests/fixtures/transcripts operands
sh tools/toolchain/sync.sh pull crates/orbistoun-shader/data/opcode-operands.toml \
    crates/orbistoun-gen/tests/fixtures/transcripts
```

Pull immediately, and pull by name. A whole-tree copy back would carry the VM's `Cargo.lock`
and overwrite whatever else was edited meanwhile.

## Recordings

- A recording carries its own input, and replay checks it. A solver whose probe list changed
  since the recording would otherwise be handed old answers to new questions, which shows up
  as a wrong table rather than a stale recording.
- Keys are derived from the probe text, not from call order, so a recording matches by what
  was asked. Under a single shared key, replay hands every call of a multi-call probe the
  same answer and whole families drop out of the operand table.
- Recording is a separate act, not a cache. A cache decides for itself when it is stale; a
  committed recording is a decision somebody made.

## Checks

| Check | Needs a toolchain | Catches |
|---|---|---|
| `./bin/orbistoun tables` | no | a table edited by hand, or a solver that changed what it produces |
| `tests/rendering.rs` | no | a formatting change in the renderer; a `.gcn` disagreeing with its `.txt` |
| unit tests in `solve`, `table`, `patterns`, `operands` | no | the bit arithmetic, including the silent-failure cases below |
| a live run in the VM | yes | everything above, against the real reference |

The solver cases that produce a wrong table rather than an error, each pinned by a unit test:
a rejected probe shifting the input pairing of every later sample; a partial trailing word
padded rather than dropped; an operand accepted with different bits recorded as implicit; and
an implicit slot at word 0 bit 0 colliding with a real field there.
