# D952 - Computed accesses through one unwritten descriptor share a buffer

**Status:** decided
**Date:** 2026-10-24
**known_by:** published

A buffer access whose descriptor or base the program computes (D753) has been its own slot, keyed
by the access's byte. Two such accesses in one basic block now share a slot when they read the
same registers - the descriptor's four or the base pair's two, and the scalar offset's register -
and no instruction between them may write any of those registers. The later access then reads the
words the earlier one read. In a basic block, the later instruction runs exactly when the earlier
one has run, with nothing in between, so the descriptor a run up to the earlier access finds is the
later access's too. The shared slot's source is the earlier access's. It reaches what both
accesses reach, as two accesses through one traced descriptor do (D733).

Which registers an instruction may write is what the tracer already forgets for it, found by
carrying a state in which every register holds a mark that no instruction forms except by moving
it. A scalar offset in `m0` or a lane mask, which the tracer holds nothing for, keeps the access
its own slot. A `null` offset or an inline constant reads no register.

A shader that still traces more buffers than a stage binds is refused as before. The refusal now
says how many of them are computed.

**Why:** Grand Theft Auto V's compute program at `0x74005b858800` was refused for 127 buffers,
more than the 64 a stage binds. A computed descriptor read by several accesses gave each access a
slot of its own, although they all bind the same range.

**Rejected:**
- Sharing across blocks wherever the same definition reaches both accesses. In a loop, a path from
  one access back through the definition to the other carries a newer value, and the earlier
  access need not run before the later one, so a run up to it can miss the later access's
  descriptor.
- Raising the 64 bindings. Every module declares the array, so every module and pipeline grows,
  and the slots would still be spent on copies of one range.
