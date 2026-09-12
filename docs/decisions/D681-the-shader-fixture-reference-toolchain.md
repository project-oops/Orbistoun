# D681 - the shader-fixture reference toolchain is LLVM 18, reachable in a container

**Status:** decided
**Date:** 2026-09-12

## The choice

The reference assembler/disassembler that `orbistoun-gen fixtures` compiles the shader
fixtures with is **LLVM 18**, and the way to run it without the documented multipass VM is
the stock `silkeh/clang:18` container plus the generator's own `--transcript` replay mode.
LLVM 19 is **not** interchangeable: it reproduces the fixtures with a different register
allocation and churns every one of them.

## Why it matters

The fixtures under `crates/orbistoun-shader/tests/fixtures/` are committed, so the tests need
no toolchain - but *regenerating* them (to add coverage, e.g. a new instruction) does, and
`tools/toolchain/setup.sh` builds a multipass VM with Ubuntu 24.04's apt `llvm clang`, which
is LLVM 18. That VM was not present on this machine, and standing one up is 4 GB and several
minutes for a job a container does in seconds.

The version is load-bearing, not incidental. Disassembling the existing `arith.ll`:

- **LLVM 18.1.8** emits `s_load_dwordx4 s[0:3], s[4:5], null` - word `0xf4080002`, byte-identical
  to the committed `arith.gcn`.
- **LLVM 19.1.7** emits the same instruction against `s[6:7]` - word `0xf4080003`. The kernarg
  segment base moved between the releases, so a regen on 19 rewrites the first word of every
  compute fixture even though nothing about the source or the target changed.

A wholesale regen on the wrong version would land dozens of spurious byte diffs mixed in with
whatever real change prompted it, and the diff could no longer be read as "this is what the new
instruction added".

## How a regen is done here (no VM)

1. Add or edit a source under `tools/shader-fixtures/` (`minmax.ll` was the first user of this).
2. In `silkeh/clang:18`, for every source, build with `llc`/`llvm-mc` on the source's declared
   triple (`amdgcn-amd-amdhsa`, or `amdgcn-mesa-mesa3d` for graphics) and `-mcpu=gfx1030
   -mattr=+wavefrontsize64`, then `llvm-objdump -d` with the same triple/mcpu/mattr. Capture
   each disassembly to `<recdir>/fixtures-<stem>.out`. That file is exactly what the generator's
   `--transcript` mode reads.
3. Regenerate natively with `cargo run -p orbistoun-gen -- --transcript <recdir> fixtures`,
   pointing `--out`/`--mnemonics` at a scratch dir first and diffing against the committed tree.
   The diff must be empty except for the intended change; anything else means a version or
   triple mismatch in the recordings, not a real result.

This keeps the provenance the generator promises: the reference still decides the bytes and the
boundaries. It only moves *where* the reference runs, and records that the recording is a
faithful transcript of it (which the empty diff proves).

## Provenance note

The container image is a convenience for running LLVM, not a source of truth about the target -
the bytes it produces are checked against the committed fixtures before anything is trusted, and
correcting a decode is still done from the published ISA, never by reading LLVM's tables (D085).
