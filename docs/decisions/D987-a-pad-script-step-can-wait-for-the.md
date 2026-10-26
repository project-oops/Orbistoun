# D987 - A pad script step can wait for the guest's first heavy frame

**Status:** decided
**Date:** 2026-10-25

A pad script step may name `wait_for_draws = N`. It takes effect at its own time or later, at the
first pad read after the guest has closed a frame of at least `N` draws, and every step after it
counts its time from that moment. The steps before it run until it takes effect, whatever their
own times, so a menu path pressed every few flips stops where the phase begins. The renderer hands
the script each closed frame's draw count at the flip, and the run logs when a wait takes effect.

**Why:** a phase such as a title's first in-world frame falls at a different flip from run to run
(a first launch against a saved profile, a slower load), so an absolute `at_flip` lands its
in-world input in a menu or a loading screen. A world frame is orders of magnitude heavier than
any menu's (PPSA04263: menus up to 78 draws, the North Yankton interior 18000 and more), which
marks the phase without knowing anything about the title, and the time after it is still counted
in the guest's own flips (D721).

**Rejected:**
- A guest memory word as the condition: it needs a title-specific address and a mapped-range check
  on every pad read; it can be added beside this when a title's state word is known.
- Re-sorting or merging the counts: a step after a wait is relative to it, and refusing an
  unordered count keeps the file saying what runs.
