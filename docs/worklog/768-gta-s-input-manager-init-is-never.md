# 768. GTA's input-manager-init is never entered, not a thread race; it is upstream-gated by int 0x41 resource checks, so the wall is a startup-reachability problem to bank while the loop broadens

**2026-09-21** — worklog 767 left PPSA04263 (Grand Theft Auto V)'s null-vtable as a gate-versus-threading
fork with a cheap watchpoint to split it. This split it: the input-manager-init is **never entered
before the fault** - reachability, not a late-thread race - and the climb above it hits the same `int
0x41` abort family the rpf.cache saga did. That makes the remaining work a deep startup trace, so this
banks it with a precise resume point and turns the loop to a wall that can actually move.

## The split, decided

A write-watchpoint on a global the input-manager-init writes near its top (`image+0x5d73438`, set at
`image+0x197be69`) came back **`never touched`**. The run faulted where it always does
(`image+0x19676d7`, the null-vtable poll) on the same thread `0x5e2d00049b00`. Because the crash halts
every thread, "never touched" means the init did not run *at all* before the poll faulted - so this is
**not** the threading case (init running late on another thread) but the **reachability** case: the init
is never called on the path that leads to the fault.

(The run report's "`scePthreadMutexUnlock` answered `0x0`, dereferenced here" line is its last-call
heuristic misfiring on the four pad objects' own mutexes at `image+0x5aaee30..0x5aaf1e8`; worklog 759
already disassembled the fault as a null-vtable virtual call, not a mutex return. Noted so the next
session does not re-chase it.)

## What the climb shows

The input-manager-init (the one function that calls the bring-up at `image+0x1965c00`, via
`image+0x197bf38`) is built from repeated resource-creation checks of the shape

```text
call 0x33911d0                 ; create a handle into [rbx+0x120]
mov  rcx, [rbx+0x120]; inc rcx
test eax, eax; cmovne rcx, r14 ; eax != 0 (failure) -> rcx = 0
test rcx, rcx; jne ok
int  0x41                      ; abort on failure
ok: ...
```

`int 0x41` is GTA's abort trap - the same family the rpf.cache misroute tripped. The current run does
**not** hit `int 0x41`; it reaches the null-vtable instead. So this init function is not aborting
part-way - it is **not entered at all**. The divergence that leaves it unreached is upstream, in GTA's
startup sequencing, several call levels above the poll.

## Why this is banked, not abandoned

The wall is now precisely stated: **GTA reaches its update/poll loop without its startup having run the
input-manager-init**, and the init is never entered (not skipped by one local flag, not run late on
another thread). Resolving it means tracing GTA's startup to find where the sequence that should reach
`image+0x197bf38`'s function diverges - or whether the main thread is parked in a wait while the update
thread runs ahead - which is a multi-level trace, not a one-symbol fix. That is real work with an
uncertain number of steps, and eight consecutive ticks have characterised this one title without the
wall moving since the rpf.cache fix.

Principle 11 (highest-payoff path) and the loop's own remit - *retail titles*, plural - say to spend the
next ticks where a wall can move measurably, and return here with the startup trace as a named task. The
resume point is exact: find the caller chain that should reach the input-manager-init, starting from its
enclosing function's entry (below `image+0x197bc40`) and its callers, and correlate with the main
thread's state at the fault.

## Gate state

No code changed - a characterisation that decides the gate-versus-threading fork (reachability), records
the `int 0x41` resource-check shape upstream of the poll, and banks GTA's wall with an exact resume point
so the loop can broaden to a measurable target next. `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
