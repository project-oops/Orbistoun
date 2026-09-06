# 416. Seventy-eight percent of every call

**2026-09-04** - directed, while the Agc probes are out for hardware

## What was found

Went looking for work independent of the console and asked the obvious question - what is
PPSA25872 doing with 11.5 million calls at 3% standing? - and the answer turned out to be the
largest single fact in the project's own work list.

```text
      CALLS  SHARE  MODULES  IMPORT
   11272991  78.3%        1  libkernel_sync_on_address::0xbd04891e6902ce1d
```

**Seventy-eight percent of every guest call this project has ever recorded**, across 65 runs, goes
to one function - in one title, on one address, and **it has no name**.

`worklist` had been reporting this all along. Nobody had read the top line.

## What was measured

Answering it with success makes things **worse**, which is the useful half:

| | placeholder | answering 0 |
|---|--:|--:|
| calls | 11,583,204 | **19,999,997** (budget exhausted) |
| imports | 129 | **78** |

So no return value fixes it. The same shape as D556's shader wall: **the mechanism has to be
modelled, not answered** - a wait that returns without waiting is a busy loop by construction.

Not implemented, because `libkernel_sync_on_address` is the futex family and **wait and wake need
opposite behaviours**. Getting it backwards would not be a slow emulator, it would be a guest whose
synchronisation is inverted, and with one caller and one address the run would look plausible
either way.

`orbistoun-cli names` is running against the title - it proves a name by hash, so a hit is proof
and a miss costs only the sweep. Seven hand-tried candidates missed.

**Neither repository knows this library exists.** The NID appears nowhere in obSCEne, no hardware
report mentions `sync_on_address`, and orbistoun declares no such module. A family both projects
overlooked accounts for more guest calls than everything else combined.

## And a correction to this morning's D564

D564 stated as a new rule that *"a function whose answer is arithmetic cannot be left unimplemented
safely"*. **That rule already existed and was already wired.** `Returns::Count` carries it word for
word, `Returns::stub_value` answers zero for it, and the service consults that on every
unimplemented call.

The Ult sizers answered the placeholder for a duller reason: **they had no knowledge entry at all**,
so there was no `returns` to consult.

That makes the finding larger, not smaller. **The safety net only covers functions somebody has
written down** - and across every trace on this machine, 172 distinct functions are called and
unimplemented while **97 of them carry no `returns` classification**.

Classifying them from their names is not the fix, and the data says so: of the four whose names
look size-shaped, `sceAgcDcbSetIndexSize` is a **setter** and
`sceKernelAprResolveFilepathsToIdsAndFileSizes` returns a status. Half the name-shaped signal is
wrong, which is exactly D356's argument against classifying prose.

## The habit worth keeping

Two corrections to my own decisions in one day - D559's Class A, and now D564 - both found by
reading what this repository already contained rather than by anything new arriving. The pattern
is identical each time: a conclusion written from a reading, when the file that settles it was one
grep away.
