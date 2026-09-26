# D139 - The shader target is RDNA2, keyed by name

**Status:** decided
**Date:** 2026-09-26

The shader subsystem targets the RDNA2-derived GPU of the current hardware generation. Supported,
blocked and dispatched instructions are keyed by name, resolved once per instruction; an opcode
with no recorded name is refused. Names come from the generators, merged with a conflict check.

**Why:** opcode numbers move between generations, and a list of numbers aimed at another
generation binds silently to whatever occupies them. Names mostly survive a retarget, and those
that do not arrive as an exact list of what needs attention.

**Rejected:**
- Family and opcode pairs: silent misbinding on retarget.
- Deriving widths from opcode arithmetic: an ordering that holds by coincidence.
