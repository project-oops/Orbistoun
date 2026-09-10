# 425. What the guest opened, and the two devices it wanted

**2026-09-07** - directed, continuing 424

## What was done

**`/dev/random` and `/dev/urandom` are served.** The device module keeps a short list on the
rule that a device answering plausibly is worse than one that is absent, and its own note had
put these on the wrong side of it. The argument that moves them is that a random device's
contract is that its bytes carry no meaning: there is nothing to invent, which is what
disqualifies the other candidates. Both names, one device, never blocking - FreeBSD's
arrangement, citable because the target kernel is FreeBSD-derived (D578).

The generator moved to `orbistoun_core::entropy`, so the devices and `std::random_device` draw
from one pool instead of two copies. It is **deterministic on purpose** - the emulator-wide trade
that makes two runs comparable - and that limitation is stated in the module rather than left to
be discovered.

**`ORBISTOUN_TRACE_OPENS` records what a guest opened**, the half `wanted` cannot show. Gated,
because failures are rare and successes are not: a lock and a string per open on the guest's own
stack is an observation heavy enough to change what it observes. Declared a setting rather than a
diagnostic - it changes what is reported, not what the guest does, so it earns no caveat and
poisons no record.

## It answered the question it was built for, and corrected yesterday's entry

```text
orbistoun: the guest opened 5 paths:
  /app0/Media/Metadata/global-metadata.dat
  /app0/Media/boot.config
  /app0/Media/globalgamemanagers
  /app0/debug.log
  /dev/urandom
```

D576 inferred from four missing probes that PPSA03416 never reached the loose-file layout.
**It reaches it.** `globalgamemanagers` is that layout's entry point and the guest opens it,
with the IL2CPP metadata and the boot config. The four missing paths were the archive layout the
title does not use - red herrings throughout - and only a record of the *successes* could show
that. The entry is corrected.

`/dev/urandom` was opened by two titles on the first run after it existed.

| Title | before | after |
|---|--:|--:|
| PPSA02664 | 197 imports | **198** |
| PPSA03416 | 192 | **193** |
| PPSA25872 | 141 recorded | 140 reached |

## Surprises

- **Five opens, one read of zero bytes.** That is now the question rather than the mystery. The
  guest opens exactly the right files and reads essentially nothing out of them.
- **A run that ends on the budget or the clock prints no filesystem report at all.** Both call
  `std::process::exit` directly, so `paths_wanted`, `syscalls_asked_for` and the new record are
  all skipped - which is why PPSA25872 shows nothing above. Pre-existing, found by adding a
  reporter and noticing it was silent where the others were also silent. Not fixed: reporting
  from the watchdog would read locks a running guest may hold.
- **The wiring had two sites and I found the second by testing.** The fault path and the ordinary
  exit path each call the reporters, and adding only the first made the new one work for a title
  that crashes and not for one that stops cleanly.
- **A test caught a false assertion in the generator.** `splitmix64`'s finaliser maps zero to
  zero - every step is a multiply or shift-xor of zero - and the first version of the test
  asserted it must not. The property is now asserted the right way round so nobody "fixes" it.
- **PPSA25872 reaches 140 where its record says 141.** Serving the random devices changed which
  path it takes. The record only moves up, so it keeps 141; the difference is a different route
  rather than a regression, and it is written down rather than smoothed over.

## Next

- Which of the five files the guest reads from, and what it does with the bytes. This is the
  live question for PPSA03416 and the record now makes it askable.
- The budget and clock exits losing every report, with the locking hazard that makes it more
  than a two-line fix.
- `/dev/null` and the rest still have no argument made for them, and should not be added until
  one is.
