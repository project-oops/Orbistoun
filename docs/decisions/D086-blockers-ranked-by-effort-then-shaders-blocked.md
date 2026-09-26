# D086 - Blockers ranked by effort, then shaders blocked

**Status:** decided
**Date:** 2026-08-20

The shader worklist sorts blockers into two effort tiers - ordinary work, then work waiting on a
subsystem - and ranks by shaders blocked within each tier, with occurrence count as a
tiebreak. Undecodable and untranslatable instructions are reported apart.

**Why:** an instruction appearing ten thousand times in one shader blocks one shader. Ranking
by what helps most ignores what is reachable, so a week of work would sit above an afternoon's.
The tier comes from the translator's existing blocked list, so there is no second table.

**Rejected:**
- Raw frequency: rewards one heavy shader.
- A blocked-over-effort score: precision invented from a guess.
