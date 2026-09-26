# D681 - Shader fixtures are regenerated with LLVM 18

**Status:** decided
**Date:** 2026-09-12

The reference assembler and disassembler that regenerates the shader fixtures is LLVM 18, run in
the `silkeh/clang:18` container and replayed natively through the generator's `--transcript`
mode. The pin is independent of the collection's build compiler, and the collection toolchain
check deliberately skips orbistoun.

**Why:** the fixtures are committed bytes, so the reference version is part of the expected
output rather than a means to it. LLVM 18.1.8 and 19.1.7 already disagree on the first word of
every compute fixture because the kernarg base register moved, and a regeneration on any other
version buries the intended change in churn. The container runs the reference only; a decode is
still corrected from the published ISA, and a regeneration is accepted only when its diff is
empty apart from the intended change.

**Rejected:**
- Following the build compiler to clang 21: rewrites every compute fixture for reasons unrelated
  to the source.
- A virtual machine per regeneration: the same bytes at gigabytes of cost.
- Moving the pin inside another toolchain change: a move needs its own reason, a fixture LLVM 18
  cannot assemble or a decode it gets wrong.
