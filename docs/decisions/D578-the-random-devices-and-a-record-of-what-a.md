# D578 - The random devices, and a record of what a guest actually opened

**Status:** measured
**Date:** 2026-09-07

## Two gaps, and they turned out to be one question

`orbistoun-fs` records every path it could not answer and calls that *the filesystem's most
useful output* (D387). It is - and it only names what was **missing**. PPSA03416 opened what it
asked for, performed **one file read of zero bytes** against a title directory holding four
hundred megabytes of assets, and the only visible evidence was four paths it probed and did not
find. All four are the archive layout this title does not use, so all four were red herrings, and
what it *did* open was recorded nowhere (D576, D577).

So the two asks are the same ask: serve the devices a guest names, and record what it got.

## The random devices earn their place on an argument, not on demand

`device.rs` keeps a deliberately short list, because *a device that answers plausibly is worse
than one that is absent - the guest cannot tell*. Its own note put `/dev/random` on the wrong
side of that line, and the argument that moves it is specific:

**A random device's contract is that its bytes carry no meaning.** There is no layout to invent
and no vendor semantics to guess, which is exactly what disqualifies most candidates. It can be
served without inventing anything, which `/dev/null` and the rest still cannot claim.

Both names, one device, and it never blocks - FreeBSD's own arrangement, and the target kernel is
FreeBSD-derived, so that is a citable shape rather than a preference.

**What it hands back is deterministic, and that is the emulator's trade rather than the device's
shortcut.** Every measurement here rests on two runs of one build behaving identically (D181,
D238); a guest seeded from a physically random source takes a different path each time and the
difference between two runs stops meaning anything. `std::random_device` has answered on that
reasoning since it was implemented. The generator moved to `orbistoun_core::entropy` so both draw
from one pool, as two readers of one kernel pool would, instead of from two copies that drift.

**A guest doing cryptography against this is not getting cryptography.** Nothing observed does.
If something starts to, that is a decision to take then rather than a property to rely on quietly
now - which is why it is written here and in the module rather than left for a reader to find.

A write is refused. On FreeBSD it stirs the pool; this pool is a fixed sequence with nothing to
stir, and accepting the write would claim an effect that does not happen.

## The record is off by default, and the failures are not

`wanted` records unconditionally because failures are rare and an ordinary run pays nothing.
Successes are the common case - a title streaming assets opens hundreds - and a lock and a string
for each, on the guest's own stack, is an observation heavy enough to change what it observes
(principle 9). So it is gated on `ORBISTOUN_TRACE_OPENS` and an ordinary run pays one atomic load.

It is declared a **setting** rather than a diagnostic, and the distinction is the one the two axes
exist for: a diagnostic changes the program in order to learn from the difference, and a verdict
under one carries a caveat. This changes what is *reported*. The guest cannot tell it is on, so it
earns no caveat and poisons no record - the same place `ORBISTOUN_FINDINGS` sits.

Recorded on the guest's stack, printed after the guest has stopped, capped at 256 distinct paths:
the same three rules `wanted` follows, for the same reasons (D381).

**An empty list and a list nobody asked for are different findings**, so a run that was recording
and opened nothing says so in words rather than printing nothing.

## What it said the first time it ran

```text
orbistoun: the guest opened 5 paths:
  /app0/Media/Metadata/global-metadata.dat
  /app0/Media/boot.config
  /app0/Media/globalgamemanagers
  /app0/debug.log
  /dev/urandom
```

**This overturns the reading in D576.** That entry inferred, from the four missing probes, that
the guest never reached the loose-file layout. It reaches it: `globalgamemanagers` is the loose
layout's entry point and the guest opens it, along with the IL2CPP metadata and the boot config.
The four missing paths were never the problem, and only a record of the successes could show it.

`/dev/urandom` is opened by both PPSA03416 and PPSA02664 on the first run after it existed, which
is as direct a confirmation as the gap being real gets.

Both titles moved: PPSA02664 197 to 198 imports, PPSA03416 192 to 193.

## What this does not establish

**Corrected by D595: the read was not zero bytes.** It was four hundred and two, complete, of
`boot.config` - `0 KiB` is what integer division makes of that. The guest opening
`globalgamemanagers` and never reading it through the descriptor is what survives, and it is
measured per read now rather than read off a rounded summary.

**Why five opens produce one read.** That is the question the record was built to expose and it is
now exposed rather than answered. The next measurement is which of those five the guest read from
and what it did with the bytes.

**Nor anything about a run that ends on the budget or the clock.** Both call `std::process::exit`
directly, so such a run prints no filesystem or syscall report at all - not this one, not
`paths_wanted`, not `syscalls_asked_for`. That is pre-existing and it is why PPSA25872 shows
nothing here. Fixing it means reporting from a watchdog thread while the guest is still running
and may hold the very locks being read, which is a hazard worth its own entry rather than a line
added in passing.

**Nor that the deterministic stream is enough.** It is enough for a guest that seeds a generator
or salts a hash, which is all anything observed does. A title that checks its randomness for
statistical quality, or one that relies on two runs differing, would find neither - and would be
right to.
