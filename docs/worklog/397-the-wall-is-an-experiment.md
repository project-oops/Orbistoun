# 2026-09-04 - (/loop) The wall is an experiment, and there is no guest to run

> **Corrected the same day by [worklog 404](404-the-wall-was-real.md) and D555.**
> There are seventy-one guest binaries; I had read the overlay root as the library. And the
> wall is honest: a zero-override run reaches `image+0xf56e09` after 420,611 calls. The
> honest slot said 222 only because it had been unwritable since 2026-08-26.

```
the wall = the [experiment] slot, overrides = 1.  Honest: image+0xafc959, 222 calls, 2026-08-23
title library: 144 directories, 12 files, all twelve guest OUTPUT.  Nothing runnable.
measured-hardware axis closed (D543-D547)
```

Thirty-fourth cron tick. The standing instruction is *re-derive every number*, and the one never
re-derived was the one in its own header.

## The wall is the helped slot

`compat/PPSA02664-app0.toml` keeps two records. `[status]` - the honest one - is
`image+0xafc959`, 23 imports, 222 calls, measured **2026-08-23**. `[experiment]` is
`image+0xf56e09`, 197 imports, 420,273 calls, **`overrides = 1`**, measured 2026-09-03.

The loop has been quoting the second as the state for about twenty ticks, without the qualifier.
The slot routing itself is D312/D323 working as designed - a helped run can never overwrite the
honest number, and `compat list` prints the honest one. The trace corroborates the experiment
independently: 418,903 calls, 197 distinct.

D227 already says an intervention that moves a wall is not a diagnosis. The same goes for one
that moves it three orders of magnitude and then gets quoted as where the project is.

The honest record also predates the fixed-base heap (D513), the unrun initialiser arrays (D514,
D515), the flip-pending fix (D516), `sceKernelStat` and `sceKernelPread` (D525, D526) - so it is
probably understated, and knowing which way it is wrong is not knowing.

## And it cannot be re-derived here

The title library holds **144 directories and 12 files, and all twelve are guest output** -
obSCEne's own `report.txt` and `boot.txt` from past runs. No `eboot.bin`, no `.elf`, no guest
executable of any kind.

So `./bin/orbistoun run <title>`, which this project's notes call *the* command, **cannot be
turned at all in this session** - not for the wall's title, and not for the twenty-two pinned
corpus payloads whose directories exist and whose files do not. `corpus sync` would fetch them
from a public GitHub release; that is a download, so it is named as the unblocker rather than
done unasked.

## What it explains

Ten ticks of tests and documents. The honest reason is not that they were the best work
available - it is that the work which *measures* progress was unavailable and nothing said so.

## The measured-hardware axis closes (D543-D547)

**Bought**: 22 measured facts asserted where none were, one divergence pinned, two stated
blockers corrected (one wrong about its mechanism, one right about its mechanism and in the wrong
list), one permanent resident removed from the work queue, thirteen blockers re-derived and found
accurate. **Cost**: five ticks and no movement on a wall that was never available to move.
**Left**: 59 outstanding entries and 25 unassertable values, every one needing a capture rather
than code - obSCEne's next step, not orbistoun's.

Decision: [D548](../decisions/D548-the-wall-is-an-experiment-and-there-is-no-guest-to-run.md).
