# D097 - Operand fields are solved from assembled probes

**Status:** decided
**Date:** 2026-09-26

`orbistoun-gen` assembles hand-written probes with the reference assembler and solves, per
opcode, the bit fields that explain every sample. Operands spelled as names and buffer formats
are measured the same way. An operand whose kind is ambiguous leaves the opcode unsolved, and a
field needs at least two samples.

**Why:** operand shape varies per opcode within a family, so a per-family layout decodes fields
that are not operands. Transcribing several hundred rows is several hundred chances to be
quietly wrong; a probe and an assembler together state the answer. Two readings of the same
bits are not a tie to break.

**Rejected:**
- Per-family operand layouts: wrong for most families.
- Transcribing operand fields from documentation: uncheckable at this volume.
- Breaking ambiguity by preference: a coin toss recorded as a fact.
