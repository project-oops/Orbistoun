# D357 - An experiment declares how its result is read

**Status:** decided
**Date:** 2026-09-26

An experiment names both the axes to run and a `Reading` that decides the question from
recorded numbers, or `Undecided` with a reason. A run records the memory map it actually built
in `Conditions::memory_map`. An answered question becomes an inert proposal against the entry
that asked it, with `known_by = "measured"`, and is not proposed again once settled.

**Why:** fault and reach cannot say whether a guest accepted a map; the offset it queries next
can, and that is arithmetic. A run that could not decide must not be recorded as a negative.
Recomputing the map from its configuration is a second copy of what is measured and wrong when
a shape falls back. Re-proposing a settled answer fills `patches/` with noise. Questions about
correctness still need the conformance probe or a person.

**Rejected:**
- Reading every experiment through the generic diagnostics: answers a different question.
- Recomputing the presented map: disagrees when a shape falls back.
