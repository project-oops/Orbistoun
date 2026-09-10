# 430. The hardware answered, and the loop reads structures by itself

**2026-09-08** - directed, continuing 429

## What was done

**The probe sections written on 2026-09-07 ran on the console**, and the report is in
`obscene/reports/hardware/fullguard-klog.txt`. `031-stackattr` passed all three checks and turned
two of orbistoun's assumptions into measurements - the stack address a thread attribute set
reports is the lowest byte, and a fresh set answers zero for it. It also found one orbistoun had
wrong: a fresh set reports a stack **size** of `0x10000`, where this project answered zero
(D585). `032-syncaddr` established that a wake with nobody waiting answers `0x0`; its wait cases
were skipped after hanging a previous run.

**Eight sites handed the guest a host heap address, and D582 had fixed one.** Thread attribute
sets, mutex and condition attributes, semaphores, synchronisation objects and the system version
block were all `Box::leak`. They come from one region now (D584).

**`Step::ReadStructure` closes the join that was left open yesterday** (D586). The loop reads back
the structure an unimplemented call was handed, with the address taken from the finding's own
evidence - including, unprompted, the command buffer whose address was typed in by hand the day
before.

## The Ampr names were already known

The hardware census lists the whole `libSceAmpr` surface, including `GetSize`,
`GetNumCommands` and `GetCurrentOffset` - which name the fields read out of PPSA03416's command
buffer in worklog 427. Every one of those names is **already in `symbols/generated.json`**, so
there was nothing to add: the naming loop had them, and what the report contributes is corroboration
of the field reading rather than a name.

`docs/PROVENANCE.md` settles the question the report raises: a name a conformance probe reported
is `runtime` evidence at the hardware tier, and the hash stays the only oracle. Nothing was taken
on the strength of appearing in a report.

## Three things had to be fixed for one step to be honest

Each was a message asserting something the code did not do, in the path of a single new step:

- The `Unimplemented` finding carried **no arguments** - the shim rendered them at print time, so
  nothing reading a finding programmatically could see the call had been handed a structure.
- The turn **discarded the worker's error stream**, while `Taken::Watched`'s documentation said
  what it saw was on it.
- The snapshot ran **before** the readable spans were published, so a watch on an image address -
  mapped since the module was placed - said it "did not exist when the guest started".

## Surprises

- **A single pair of runs detects non-determinism and does not measure it.** The first pair after
  the block-allocator change gave 1 of 88 identical mappings and read as a regression; the real
  figure is 28 of 88 against the host clock's 1, and how far two particular runs agree varies on
  its own. A wrong conclusion was held for several minutes on one sample.
- **The hardware-measured attribute default appeared to cost an import** - 192 against 193. Four
  runs give 192, 192, 193, 192: it is the drift `CheckRepeats` measures, not a cost.
- **The mapping count roughly doubled** across the block-allocator change, wall unchanged. Handles
  are consecutive addresses now rather than scattered heap ones, and something about the guest's
  behaviour changed with them. Not diagnosed.

## Next

- Why the mapping count doubled when handles clustered. It is the kind of thing that is either
  meaningless or the whole answer.
- The command buffer read at *stop* differs from the one dumped at the *call*; reading it at the
  call is a different mechanism than a snapshot.
- `032-syncaddr`'s wait cases still hang the probe. They are the half that would settle the futex.
