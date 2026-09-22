# 796. A full-code scan finds zero static references to PPSA04263's object, confirming computed-dispatch (tracer-bound); with the retail frontier exhaustively tool/hardware-bound, the loop shifts from wall-moving to honest hardening of real exports

**2026-09-22** — worklog 795 returned PPSA04263 to the execution-divergence class and named a static
construction-finder as the last non-tracer avenue. This tick ran it and it came back empty, which settles
PPSA04263 and, with it, the shape of the whole session's remaining work.

## Zero static references

A byte-scan of the eboot's entire executable segment (`0x0..0x3395a4c`, ~54 MB) for any RIP-relative
instruction whose target is `image+0x5b37e98` found **none**. So no instruction reaches the object by a
fixed address: it is reached only through computed addressing (`base + index*stride`, as the manager
array itself is), and its constructor is dispatched the same way. Static analysis cannot follow it - the
same wall ASTRO BOT and GTA's earlier sites hit. PPSA04263 is tracer-bound, confirmed from the object's
own address rather than inferred.

## The frontier, settled across twelve ticks

Since worklog 787's region-return moved PPSA28061 twice, twelve ticks have tested every buildable-looking
lead on the six retail titles and each resolves to a capability the single-tick loop does not have:

- **PPSA02664 / PPSA03416** (Unity-AGC): `array[1]+0x18` null - AGC descriptor (789), Ampr command
  buffer (791), module inits and zero-fill all ruled out; workload-array state, obSCEne REQ-...7a5d.
- **PPSA28061** (AGC): register defaults answered (787), now the hardware-faithful mapper (obSCEne a2f9).
- **PPSA04263** (GTA), **PPSA21564** (ASTRO BOT), **PPSA25872** (Terminator): un-run construction or
  assert reached by computed dispatch / metadata indices; static RE exhausted (this worklog, 784, 771-775).

Every one needs an **execution/branch tracer from entry**, a **reference-emulator diff**, or a **pending
obSCEne measurement**. None is a single-tick wall-move, and re-confirming that is no longer progress.

## The shift: harden what the loop can, honestly

Wall-*moving* is exhausted, but the loop's purpose - harden the retail titles - still has honest,
buildable work that does not need a tracer or hardware: **real exports the titles call that orbistoun
answers with a placeholder**, implementable from published POSIX/FreeBSD semantics or from the presented
machine. A placeholder a guest consumes is a lie (principle 3) regardless of whether removing it moves a
particular wall, and each honest answer both improves fidelity and, occasionally, unmasks a movable path.
PPSA04263 alone has `scePthreadGetaffinity` (a `pthread_getaffinity_np` analogue - orbistoun already
stores a thread's `requested_affinity`, D523 anticipated the read), and others across the titles.

So the loop now works that list rather than re-deriving the tool-bound walls, at a heartbeat cadence
rather than a frantic one, and picks the wall grind back up the moment an enabling build or an obSCEne
result lands. The tool-bound walls are not abandoned - they are correctly attributed as orbistoun's
(D708) and waiting on a capability, which is a truthful state, not a "faithful-failure" verdict on a
title.

## Gate state

No code changed - a scan that finds no static reference to PPSA04263's object (computed-dispatch
confirmed), a consolidation of the twelve-tick frontier survey, and a stated shift from wall-moving to
honest export hardening. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean.
No commit.
