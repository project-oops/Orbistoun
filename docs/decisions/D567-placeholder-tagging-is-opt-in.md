# D567 - Placeholder tagging is opt-in

**Status:** decided
**Date:** 2026-09-04

Under `ORBISTOUN_TAG_PLACEHOLDERS` an unimplemented function answers `0x7fff_0000 | (0x10 + its
stub slot)`, so the value names its source; an ordinary run answers `0x7fff_0001`. The variable
is `Effect::Intervenes`, tags start at `0x7fff_0010`, and a decoded tag counts only when it
matches an import the run called. Precedence is override, then tag, then declared return.

**Why:** a placeholder found in a guest argument is otherwise unattributable, and a finding that
sends a reader searching must carry what to look at. Much recorded evidence fixes arities by
spotting `0x7fff_0001` in a register, so a new default would stale it at a stroke. The floor keeps
the fixed `GuestError` codes from being read as tags, and the called-import check filters stale
register contents that land in the tag range.

**Rejected:**
- Tagging by default: invalidates every recorded fact that reads the plain placeholder.
- Tags from zero: `0x7fff_0001` would be attributed to whatever import sits at slot 0.
- Decoding any value in range: stale registers produce confident wrong attributions.
