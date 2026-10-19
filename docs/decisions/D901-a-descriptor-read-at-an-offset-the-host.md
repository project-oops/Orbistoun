# D901 - A descriptor read at an offset the host cannot know binds its whole buffer

**Status:** decided
**Date:** 2026-10-19

The host may find a computed descriptor (D753) but not the scalar offset the access adds to it,
because the program forms the offset from its lanes, for example from `v_readfirstlane` or a lane
compare. Such an access binds the descriptor's whole buffer, from its base through its records. The
access itself adds the offset when it runs, wherever in the buffer the offset lands. A scalar load
whose base is at such an offset is still refused, because its range starts at the offset.

**Why:** Grand Theft Auto V's compute programs read a descriptor table, or a small constant buffer, at
an offset that a lane's value or a lane compare chooses. The descriptor is known to the host but the
offset is not. Binding the buffer whole costs no more than the buffer itself, and the buffer is
bounded as any range is (D733).

**Rejected:**
- Running every wave to find the offsets (D900): a dispatch of more waves than the host runs is left
  refused, and the whole buffer needs no run at all.
