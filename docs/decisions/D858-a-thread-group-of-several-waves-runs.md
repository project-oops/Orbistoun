# D858 - A thread group of several waves runs one invocation per wave

**Status:** decided
**Date:** 2026-10-14

A guest compute dispatch whose thread group is more than one wave is translated as a workgroup of
one invocation per wave. Each invocation simulates its wave's lanes as before; its wave is its
`LocalInvocationIndex`, and a lane's thread id is the wave's first thread plus the lane, split into
x, y and z as the group's shape says, x fastest. A lane past the group's last thread starts
inactive, as do the lanes a partial last group does not run (D855).

This holds only for a program whose waves never meet: one that executes no `s_barrier` and no
local-data-share instruction. A group of waves whose program does either is refused, saying how
many waves it is and which it does.

**Why:** Grand Theft Auto V's first submission stops at a compute dispatch of 1024 threads, sixteen
waves, whose program waits at no barrier and uses no local data share. The hardware runs a
group's waves as separate wavefronts that share only the local data share and the barrier
(AMD's published RDNA ISA, on the local data share and `S_BARRIER`). Without either, nothing in one wave reaches another, so separate
invocations run them exactly. A device test runs a buffer clear in one group of 256 threads and
one of 100, and every thread's slot is cleared and no other.

**Rejected:**
- Running all a group's waves serially in one invocation: it keeps a barrier's meaning, but each
  wave needs its own register file in one invocation. It is the model for a program that meets,
  once one reaches the loop.
- Running the waves as separate invocations whose `s_barrier` becomes a workgroup barrier: one
  invocation simulates a wave whose lanes branch apart in a loop, and a barrier reached on only
  some paths of that loop is undefined in Vulkan.
