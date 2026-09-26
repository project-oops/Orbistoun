# D686 - A deliberate exit outranks a fault above the import count

**Status:** decided
**Date:** 2026-09-14

`Status::ranking_key` compares how a run ended immediately after the rung and before `imports`:
at equal reach, a run that left through `exit` beats one that faulted.

**Why:** a run whose import count falls because it stopped correctly would otherwise read as a
regression, when the calls it lost were the program running past its own exit. A tiebreaker
below `imports` cannot fire for that case, because `imports` is what moved. The rung is compared
first, so a program that exits at once still loses on imports within its rung and cannot
overtake a title that flipped.

**Rejected:**
- Below `imports` with the other quality measures: never decides the case it exists for.
- No ending in the key: a correctness fix reports `BACK`.
