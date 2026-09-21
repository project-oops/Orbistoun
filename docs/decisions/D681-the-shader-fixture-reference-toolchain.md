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

## Amendment, 2026-09-21: the collection moved to clang 21 and this pin did not

**Still LLVM 18. Deliberately, and this section exists so that nobody has to work out whether it
was an oversight.**

On 2026-09-21 the collection's *build* compiler moved from clang 18 to clang 21 (`oops-mesa#D013`),
which changed `oops-mesa/toolchain/Dockerfile`, `obscene`, `oops-apps` and the documents that
described the pin. `silkeh/clang:18` in the procedure above was not swept along with them.

**Because it is not the same kind of pin.** A build compiler is asked to produce a working
binary, and any version that manages it is doing its job. This one is asked to reproduce
*specific bytes* that are already committed, so the version is part of the expected output
rather than a means to it. The two happened to be the same number until today; that was a
coincidence of scheduling, not a constraint, and the sentence above about the image matching
the collection's compiler was never the reason for choosing 18.

The evidence that the distinction is real is already in this entry: **18.1.8 and 19.1.7 disagree
about `arith.ll`** - `s[0:3]` against `s[6:7]`, `0xf4080002` against `0xf4080003` - because the
kernarg segment base moved between two *adjacent* releases with nothing about the source or the
target changing. Three majors is not a safer distance than one. A regen under 21 would churn
every compute fixture, and the churn would be indistinguishable from a real result, which is the
failure the whole entry is written against.

**What would justify moving it** is a reason of its own: a fixture that LLVM 18 cannot assemble,
or a decode LLVM 18 gets wrong. Neither is true today. If it happens, the move is a fixture regen
reviewed as its own change, on its own evidence, with the byte diff read rather than skimmed -
not a line in somebody else's toolchain bump.

So: `orbistoun` is the one member of the collection that `OOPS/tools/check-toolchain.sh`
deliberately does not check, and this is the reasoning that gate points at.
