# D123 - An opcode with no shape row is an error

**Status:** assumed
**Date:** 2026-09-26

`Builder::check` verifies that every emitted opcode has a row in the shape table before it
checks anything about identifiers.

**Why:** skipping an unknown opcode also skips recording its result, so the next instruction
using that result is reported as referring to nothing - a false failure naming the wrong
instruction. A check that degrades on unknown input has to be asked what it does with the parts
that depend on the unknown.

**Rejected:**
- Skipping unknown opcodes: misattributed failures downstream.
- Guessing an unknown opcode's shape: reads literals as identifiers.
