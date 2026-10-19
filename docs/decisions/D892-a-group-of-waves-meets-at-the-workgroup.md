# D892 - A group of waves meets at the workgroup barrier where no branch crosses it

**Status:** decided
**Date:** 2026-10-19

A guest compute dispatch whose thread group is more than one wave and whose program waits at
`s_barrier` or uses the local data share runs as D858 runs any group of waves: a workgroup of one
invocation per wave. The waves share the local data share, which is the workgroup's storage. They
meet at `s_barrier` through the workgroup's barrier.

The dispatch loop (D098) is cut into one loop for each run of blocks ending at an `s_barrier`, and
the last run ends at the program's end. Each loop runs while the program counter is within its own
blocks. The workgroup's barrier stands between two loops at the function's top level, so every
invocation reaches it once each time, as every wave reaches its `s_barrier` once. A wave that has
ended holds the exit value, past every run, so it runs none of the later loops and still passes
each barrier.

A program with a branch from one run into another is refused by name. That is a branch over an
`s_barrier`, or a loop around one, so one wave would meet the others a different number of times.
A dispatch run one invocation per lane (D880) still refuses a group of waves that meets: its waves
are then separate workgroups, which share nothing. A group of one wave is translated as before,
one loop, byte for byte.

**Why:** Grand Theft Auto V's scene compute program reduces through the local data share in groups
of 256 threads, four waves, with eight `s_barrier`s at the top of its program. It has no branch
back, and none of its branches crosses a barrier. D858 rejected the workgroup barrier because a
barrier inside the dispatch loop is reached on only some paths of that loop, which Vulkan leaves
undefined. Between the cut loops it is reached in uniform control flow. A device test runs a group
of two waves in which each thread writes its id to the share, waits, and stores the word of thread
`id ^ 64`, the other wave's. Every word is the one the other wave wrote.

**Rejected:**
- Running all a group's waves serially in one invocation, as D858 left open: every register file
  would take a wave index, which touches every register access, and it is needed only for a
  barrier that a branch crosses, which this refuses.
- A barrier inside the single dispatch loop: undefined in Vulkan, as D858 says. A driver may hang
  it or let one invocation through early.
