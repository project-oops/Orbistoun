# D914 - A dispatch's group-decided descriptor is found group by group

**Status:** decided
**Date:** 2026-10-20

A dispatch's computed buffer descriptor may depend on nothing a wave's lanes hold, only on its
user data and its group's ids. Such a descriptor is found by replaying the scalar prefix once per
group, with the group's ids in the registers after the user data, one per dimension the dispatch
hands the program. This is how the hardware loads them. A group that branches past the access
reaches nothing; the distinct descriptors are bound as D900's waves' are.

This runs before the wave-by-wave run (D900), which runs every lane and stops at a bound on waves.
A dispatch of more than 65536 groups is not replayed. When neither run can follow the dispatch, the
refusal reports what the first waves of sixteen groups reach the access with: how many distinct
descriptors, their bases, and which registers they cannot know.

A dispatch's program that reads its own address (`s_getpc_b64`) is now told where it was loaded,
as a draw's is (D842). This applies to its translation and to the tracing of its buffers, so a
descriptor whose base is the program's own constant data is found.

**Why:** Grand Theft Auto V's loading-screen dispatch has 0x78 x 0x44 groups, far more waves than
D900 runs. It builds a descriptor from `s_getpc_b64` plus 0x18d4: a table in its own constant data.
