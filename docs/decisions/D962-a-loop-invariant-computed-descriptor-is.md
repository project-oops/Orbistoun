# D962 - A loop-invariant computed descriptor is read at its first reach

**Status:** decided
**Date:** 2026-10-25
**known_by:** guest-observed

A computed buffer descriptor is read where the dispatch's wave run (D900) stops, when the run
cannot follow the program - a loop or a block on an execution mask or condition the host cannot
know - and nothing still ahead of the run can change the descriptor before the access. The run's
frontier is where it stopped and the target of each block a way that skipped it still waits at. A
static check over the program's control flow takes every instruction on a path from the frontier to
the access: reachable from a frontier point, and able to reach the access. Where none of them writes
the descriptor's registers, every later reach of the access sees what the frontier holds, so each
frontier point the access is reachable from records its registers beside what earlier reaches
recorded. A loop that holds the access, and one between the descriptor and the access, are both
covered. An instruction with no name, a jump to a computed address, or a branch into an
instruction fails the check. Where a register is written on such a path, or no reach of the access
is known, the run keeps refusing (D900). Running out of steps is never a stopping point: a run that
could follow its loops but not within a wave's or a preparation's bound refuses as D951 says.

**Why:** Grand Theft Auto V's single-pass scan at `image+0x3e18000` builds its descriptor in
`s[0:3]` with `v_readfirstlane` at 0x458, then reads and stores through it at 0x490, 0x4b4, 0x4e4
and 0x694: inside a look-back loop, past a retry loop on a flag it loads, and after the loop exits.
The host cannot follow those loops, so the wave run refused every one of them (D758), though no
instruction between 0x458 and those accesses writes `s[0:3]`: the one value is exact. Its store at
0x6b4 follows a rebuild of `s[0:3]` at 0x6a4 and is still refused.

**Rejected:**
- Following the loops. Their conditions are loaded words, so no host run can decide them.
- Binding the descriptor whenever a probe is recorded, without the check. A loop that rewrites the
  registers reaches the access with other descriptors, which one recorded value would not name.
- Checking only the loop that holds the access. An access past a loop the run cannot leave is
  never reached, and one in no loop is never covered.
- Every instruction reachable from the frontier. A rebuild after the access's last reach would
  refuse a descriptor nothing changes.
