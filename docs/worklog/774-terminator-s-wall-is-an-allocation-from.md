# 774. Terminator's wall is an allocation from an already-bad memory pool; the root is the pool creation, not in the call stack, and Il2Cpp string tables block finding it statically

**2026-09-21** — worklog 773 planned one more frame-climb to reach the code that initiated the failing
pool build. The climb finished, and it moved the target rather than hitting it: the fault is an
**allocation from a pool that is already bad**, so the fault's own call stack does not contain the root.
It also ran into why the usual static shortcut does not work here. Both are worth recording so the next
attempt starts from the right place with the right tool.

## The full climb, and what it shows

Walking the `rbp` chain to its top (in the captured window):

```text
0x17554a3 int 0x41 (reporter)  <-  0x1755b55 (formatter)  <-  0x7b6081 (raiser)
  <-  0x7b62dd (7b6xxx mem mgr)  <-  0x49421e (string builder, allocs via 0x7b62a0)
  <-  0xbedb0c (app: virtual-dispatch, builds a string sized from a vtable[0x58] call)
```

The top application frame (`0xbeda80`) makes two virtual calls and then **builds a string**, and it is
that string's **allocation from the pool** (`0x494150 -> 0x7b62a0`) that raises the fatal error. So the
failure is not "this code did something wrong" - it is that the **pool it allocates from is already
0-sized/invalid/exhausted**. The pool was created earlier, with a bad size or base; that creation is
**upstream of this entire call stack**, not inside it. Climbing further up this stack cannot reach it -
worklog 773's plan was aimed one level too low.

## Why the static shortcut fails here

The obvious move is to find the pool by its name ("Hostname Lookup Memory Pool") and the error by its
text ("Invalid info address.") - jump straight to the creation and the check. Both come back with **zero
code references**: PPSA25872 is an **Il2CPP (Unity) title**, and Il2CPP keeps its strings in an indexed
**metadata table**, reached by table + index, never by a `lea` to the literal. So string-reference
navigation - which works on the AGC titles - does not work here, and that is a general fact about the
three Unity titles, not a one-off.

## What is and isn't established

Established: the wall is a fatal allocation failure from a named pool that is bad before this allocation
runs; the pool is DNS/"Hostname Lookup"-named; the abort is the flag-gated fatal assertion of worklog
773. **Not** established: that it is a network-HLE gap. `libSceNet` has 26 imports and `libSceNetCtl` 2,
all **unbound** (no module provides them), but the trace shows **none of them called** - so Terminator is
not faulting on a null libSceNet slot, and the pool may be an internal Unity pool merely named for DNS,
created with orbistoun's own memory. The regression tie to a8b2d74 (worklog 772) is likewise not yet
proven against this specific pool.

## The right next tool

Since the creation is upstream and the name is table-indexed, neither the call stack nor a string
`lea`-ref reaches it. A **read-watchpoint on the pool-name string bytes** (`image+0x1e54278`) would
catch the instruction that reads the name while registering the pool - i.e. the creation site - at
runtime, stepping past both obstacles at once. That is the next move, and it decides whether the pool's
bad size comes from an orbistoun value (fixable) or from the title's own logic reacting to one.

## Gate state

No code changed - a characterisation that finishes worklog 773's climb, relocates the root to the pool's
creation (upstream of the fault's stack), records the Il2CPP metadata-table obstacle that blocks
string-ref RE on the Unity titles, and names a read-watchpoint on the pool name as the tool that reaches
the creation. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
