# D098 - Predicated translation in a dispatch loop

**Status:** decided
**Date:** 2026-09-26

Shaders translate by predication: the execution mask is state, and every guest basic block is
an arm of one `OpSwitch` inside one loop, selected by a program counter. A branch assigns the
counter; a branch target that is not an instruction boundary is refused. Structured
reconstruction is a strategy that refuses loudly rather than falling back.

**Why:** the guest has no structured control flow, and irreducible flow has no structure to
recover. The dispatch loop is valid for any flow and mechanical to verify, and it becomes the
reference a structured translator is later checked against.

**Rejected:**
- Structured reconstruction first: a decompilation problem that fails subtly.
- A nested selection per conditional branch: reintroduces structure.
- A silent fallback from the structured strategy: looks implemented while running slowly.
