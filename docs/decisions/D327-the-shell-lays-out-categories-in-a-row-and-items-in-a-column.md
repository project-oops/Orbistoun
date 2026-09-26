# D327 - The shell lays out categories in a row and items in a column

**Status:** decided
**Date:** 2026-08-27

The shell shows a row of categories with the selected one's items in a column below, navigated
by a pure type over one item count per category. Nothing wraps, and pointer and pad move the
same highlight.

**Why:** the whole shell is then reachable with four directions and one button, so a controller
can operate it. Someone navigating by feel counts presses, so an extra press rests against the
end. One highlight means neither input can reach something the other cannot. The shape is
generic; no vendor artwork, motion, sound or name is used.

**Rejected:**
- Wrapping at the ends: one press too many starts a journey back round.
- Separate pointer and pad selections: the two can disagree.
