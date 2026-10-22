# D947 - Every opcode is named by sweeping the reference disassembler

**Status:** decided
**Date:** 2026-10-21
**known_by:** published

The decoder names an instruction only as the reference disassembler names it (D139). Until now its
names came from what the fixtures happened to compile, so a title using any other instruction
stopped on an opcode no table named, one at a time. `tools/shader-fixtures/sweep.py` builds, for
every family in `encodings.toml` and every value of its opcode field, instruction words whose
source fields name plain registers, and has LLVM's AMDGPU disassembler (LLVM 21, `silkeh/clang:21`,
`gfx1013` with wave64) name them. Each opcode takes the name its plainest word, every free bit clear,
decodes to. Those names join `mnemonics.toml` as names alone. An instruction a translation needs
still has its operand layout solved from samples (D139), and still needs a translation.

The sweep is checked against the names the fixtures gave: none of them differs. Forms whose name
depends on a field besides the opcode - flat, global and scratch memory by segment, an image load by
dimension - keep the names their fixtures give and are not swept.

**Why:** Grand Theft Auto V stopped on ten instructions in one day that no table named, each a run
of several minutes to find; the sweep named 767 more opcodes in seconds.
