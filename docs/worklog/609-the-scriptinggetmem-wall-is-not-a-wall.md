# 609. The scriptingGetMem "wall" is not a wall; Terminator dies at `int 0x41`, like Grand Theft Auto

**2026-09-15** - a wall I had classified turned out not to exist, and the two titles I thought had
different walls have the same one

## What I set out to fix, and why it was already right

PPSA25872 (Terminator 2D: NO FATE) prints, on every run:

> `orbistoun: the guest asked for the address of scriptingGetMem - which a title's route does not
> resolve by name, as the console does not`

I had recorded this as an OS/HLE wall - a dynamic-symbol-resolution gap. It is not a gap. The
message is a **measured behaviour** being reproduced faithfully:

- `resolves_by_name(Route::Title)` is `false`, and that is measured: obSCEne's
  `060-module/dlsym-resolves-known-symbol` asked a launched title for a known symbol from a valid
  handle and the console answered `ESRCH` (`0x8002_0003`). A title's `dlsym` does not hand out
  platform names by name (D669).
- A guest's *own* export is still resolved on a title route - `dlsym` tries `guest_export` whatever
  the route (D517). So the only way the message fires is that the name is neither a resolvable
  platform name nor one the guest's binary exports.

Both halves check out for Terminator: it asks for `scriptingGetMem`, its binary does not export it,
and the platform route does not resolve platform names - so `ESRCH` is exactly what the console
would answer. The guest takes that answer and **flips a frame anyway**. It is not stopped by it.

## The contrast that proves it, measured

A one-line diagnostic on `export_addresses`, run against both titles and then removed:

| title | exports read | `dlsym("scriptingGetMem")` |
|---|--:|---|
| Alex Kidd in Miracle World (PPSA02664) | **1** (`scriptingGetMem`) | `answered 0x400000f25330` |
| Terminator (PPSA25872) | **0** | `ESRCH`, as the console does |

`export_addresses` succeeds for both - Alex Kidd in Miracle World proves it reads the SELF-wrapped export table
correctly - and Terminator's binary simply exports nothing. So Terminator's `dlsym` is a **probe**
for a symbol it does not provide, answered the way the platform answers a probe for an unexported
name on a title route. No bug, and nothing to implement.

## What Terminator's wall actually is

The fault is `image+0x17554a3`, and the taxonomy work from worklog 605 names it without my help:

> `KERNEL ENTRY, UNIMPLEMENTED: the faulting instruction is int 0x41`

**It is `int 0x41` - the exact same wall as PPSA04263 (Grand Theft Auto V).** Two titles I had
listed with different walls (one "name resolution", one "int 0x41") have the *same* wall, a kernel
entry orbistoun does not yet service. The scriptingGetMem message was a red herring the taxonomy saw
through and I did not.

## Why this is worth more than "no change"

- **It consolidates two walls into one.** REQ-...b3c2, which measures what `int 0x41` does, now
  unblocks *two* commercial titles, not one - and the interrupt scaffolding built in worklog 608
  services both from a single `install(0x41, handler)`.
- **It is a correction, made plainly.** My earlier per-title classification called Terminator's wall
  dynamic-symbol resolution; it is not. The report was right and my reading was wrong - which is the
  same lesson as GTA's "missing asset", and exactly why the fault taxonomy was made to name itself:
  so the diagnosis does not depend on me reading it correctly.

## One thing left genuinely open, and deliberately not chased

Alex Kidd in Miracle World and Terminator both name the *identical* symbol `scriptingGetMem`, which suggests shared
middleware - so it is faintly possible Terminator *should* export it and its 0-export table is a
structural read the SELF parser handles for one layout and not another. Against that: the parser
demonstrably works on Alex Kidd in Miracle World's same-format binary, it returned a clean `Ok(0)` rather than an
error, and resolving `scriptingGetMem` would not move Terminator's wall (it flips regardless and
dies at `int 0x41`). So this is noted, not filed: if a title ever turns out to need an export the
parser missed, the lead is here, but nothing measured says one does.

## Gate state

No code changed - the diagnostic was added, read, and removed. `./bin/orbistoun worklogs` unique,
identity scan clean. The finding is the deliverable: one fewer wall, and the interrupt work already
done covers what remains.
