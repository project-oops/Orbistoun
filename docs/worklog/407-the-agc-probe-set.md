# 407. The Agc probe set, aimed from the guest's own arguments

**2026-09-04** - (/loop, then directed)

## What was done

libSceAgc became reachable on hardware, so the question became what to point a probe at. Rather
than list functions and hope, the ask was **aimed by measurement**: a run with a region planted at
`*arg0` of `sceAgcCreateShader` takes PPSA02664 past the wall - 197 imports to 215, one shader
created to **33** - and the `arg0` it passes to each of the twenty Agc functions behind it sorts
them into four classes (D559).

Three distinct pointer values do the sorting, and that is the whole finding: functions sharing one
are taking the same object.

- **Class B** (5 functions) - a `{begin, end}` writer struct on the caller's own stack, 0x38 in
  front of a 0x400 command buffer. **Constructible from nothing**, so these are safe today.
- **Class A** (9 functions) - one library-owned Dcb handle. Nothing observed constructs it; the
  guest already holds it by the first call. Identifying its constructor opens nine at once.
- **Class C** (7 functions) - a handle from an earlier call. Four of them receive `0x7fff0001`,
  which is **orbistoun's own placeholder**: the guest is feeding them the return value of a call
  nothing implements.
- **Class D** (1) - a static descriptor in the guest's image.

Written up in `docs/HANDOVER-OBSCENE.md` with counts, pointer values, what to record for each, and
what each unblocks.

## The gate, and why it needed an argument rather than a citation

obSCEne's D008 refuses to call a function of uncertain arity, and its test for certainty (D107) is
two independent open reimplementations agreeing. **That cannot be met for Agc** - shadPS4 and
GPCS4 are PS4/Gnm projects and there is no mature open Agc reimplementation to be the second
source. Read literally the gate blocks the entire ask, for ever, for the one library that matters.

The way past it is that **System V passes the first six integer arguments in registers, and a
callee ignores the ones it does not take**. Setting all six to individually safe values is safe for
any arity up to six. The stack - which is what D008 protects - is never involved.

So the risk was never arity. It is *value*: a pointer parameter handed a non-pointer. That is what
the class census bounds, and it is why the handover leads with which calls are safe rather than
which are interesting.

## The thing worth not doing

Class C looked like the richest seam - `sceAgcSetCxRegIndirectPatchAddRegisters` is the most-called
Agc function in the run at 23 calls. It is also the one to refuse: its first argument is a handle
whose producer is unidentified, so fabricating one manufactures exactly the value risk the safety
argument does not cover. Recorded as excluded, with the reason, rather than left off the list.

## Surprises

**`0x7fff0001` appearing as a guest argument named the dependency for free.** Four Class C
functions receive orbistoun's own unimplemented-placeholder in `arg0`, which says without any
further work that they are chained behind a call nothing implements. A probe list written from the
symbol names alone would have put them at the top on call count.

**The 472 `absent` Agc symbols have a recorded cause**, and it is not "no GPU library is mapped" as
this project's own notes said this morning. It is `excluded at build time: known to end the process
on this platform` - loading libSceAgc killed the probe. One capture's link-map walk did pass, and
found two modules loaded. If the library now maps, re-running the existing census converts all 472
into a real surface with no new probe code at all.

## What this does not show

That the class boundaries are real rather than incidental. Two functions sharing a pointer value in
one run of one title is evidence, not proof, and no second title in the corpus reaches this code.
