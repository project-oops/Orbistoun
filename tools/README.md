# tools

Offline generators, oracles and gate helpers. Nothing here is a build dependency: each tool
produces or checks material that is committed, so the repository builds and tests on a machine
that has none of their prerequisites.

## `shader-fixtures/`

The shader sources the decoder is tested against, generated from source and never extracted
from a guest.

| Path | Holds |
|---|---|
| `*.ll`, `*.s` | LLVM IR and assembly sources for the differential decoder fixtures |
| `families/` | one probe file per instruction encoding family, read by `orbistoun-gen encodings` (see [families/README.md](shader-fixtures/families/README.md)) |
| `probes/` | per-opcode operand probes, read by `orbistoun-gen operands` |
| `generate.sh` | runs `orbistoun-gen fixtures` from the repository root |
| `probes/run.sh` | assembles each probe with `llvm-mc` and shows the encodings, for the target `orbistoun-gen target` reports |

`orbistoun-gen fixtures` writes the `.gcn` and `.txt` fixtures under
`crates/orbistoun-shader/tests/fixtures/` and `crates/orbistoun-shader/data/mnemonics.toml`. The
reference assembler and disassembler is LLVM 18 with the AMDGPU backend (D681). It runs in the
`silkeh/clang:18` container, recording each disassembly, and the generator replays the recording
natively:

```bash
cargo run -p orbistoun-gen -- --transcript <recording-dir> --dry-run fixtures
cargo run -p orbistoun-gen -- --transcript <recording-dir> fixtures
```

`--record <dir>` captures a recording from a machine where `llvm-mc` is on `PATH`. A
regeneration is accepted only when its diff against the committed fixtures is empty apart from
the intended change.

## `toolchain/run.sh`

Runs a command inside the `orbistoun-build` multipass VM, from the repository root:

```bash
sh tools/toolchain/run.sh cargo run --release -p orbistoun-gen -- operands
```

It fails rather than falling back to the host, so a generator never runs against whichever
assembler is on `PATH` and leaves a table with unrecorded provenance.

## `validate-spirv.sh`

Runs `spirv-val` over every `.spv` module in `target/spirv/` (or `$DIR`) and exits non-zero on
any invalid module. The emitter's own tests check structure; validity needs an independent
validator. Emit the modules on the host, then validate wherever the SPIR-V tools are installed:

```bash
cargo run -q --example emit-minimal -p orbistoun-spirv -- target/spirv/minimal.spv
sh tools/validate-spirv.sh
```

## `validate-device.sh`

Runs the device tests under the Khronos validation layer and fails on any `Validation Error` in
the output, which the tests themselves cannot see. With no arguments it runs
`-p orbistoun-gpu-vulkan`; any arguments go to `cargo test` instead:

```bash
sh tools/validate-device.sh
sh tools/validate-device.sh -p orbistoun-gpu-vulkan --test console_fragment
```

`LAYER_PATH` names the directory holding the layer manifest. The test that deliberately hands the
driver a malformed module is skipped by name. Run it after changing what the emitter declares,
what the device is created with, or what a pipeline binds.

## `differential/reference.c`

Records what a published C library does - return value, `errno` and out-parameters - for a
single list of cases, emitting the inputs beside the results so the checker rebuilds each call
from the same list. The committed output is
`crates/orbistoun-hle/data/differential/glibc-2.39.txt`, which
`crates/orbistoun-service/tests/differential.rs` compares Orbistoun against.
`./bin/orbistoun check` rebuilds the program where the reference library is reachable and fails
if its output differs from the committed run.

```bash
cc -O0 -Wall -Wextra -o reference reference.c && ./reference
```

## `prose-newlines.awk`

Finds Rust string literals that continue a sentence across a raw newline, which puts the next
line's indentation into the rendered text. The `prose` step of `./bin/orbistoun check` runs it
over `crates/`:

```bash
awk -f tools/prose-newlines.awk <files>
```
