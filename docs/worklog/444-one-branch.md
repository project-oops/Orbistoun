# 444. One branch

**2026-09-08** - directed, continuing 443

## What was done

**The ninth leak is fixed and `blocks` lives in `orbistoun-mem`** (D601), which is where
guest-visible memory belongs by principle 4 and is below both subsystems that need it - so
nothing moved sideways and no dependency was added to reach it. A `FILE *` repeats now, and
`scePthreadSelf` answers `0x5e2d00000a20` in three consecutive runs.

**The count was never nine.** `orbistoun-abi` still hands the guest host heap addresses at entry;
that crate has *no* orbistoun dependencies at all, so reaching the allocator from it is a
structural change to a leaf crate rather than a two-line fix. Reverted the attempt and left the
reason at the site.

**Then localised the 192/193 drift to a single branch** (D602). The mapping record puts the whole
difference at one place:

```text
1  call 226   0x740000000000 +0x100000  asked-for  during sceKernelMapDirectMemory   <- only sometimes
```

Present means 192 imports; absent means 193; four runs, exact correlation. The reservation-failure
addresses differ between the two by exactly that mapping's size, so they follow rather than cause.

**Which needed the half the record was not keeping.** A missing mapping could be one orbistoun
refused or one the guest never asked for, and only successes were recorded - the same asymmetry
D578 and D581 closed, surviving inside the record built to close it. Refusals are recorded now,
and no run has one: **the map is never attempted**, so the divergence is a branch in the guest's
own code between calls 219 and 226.

## Surprises

- **192 against 193 was never noise.** It was used to dismiss a `FURTHER` verdict, cited as the
  motivating example in D583, and blamed for a finding that was really a configuration artefact.
  It is one guest branch with everything downstream following from it.
- **D600's headline is withdrawn.** "The determinism work landed" rested on five runs at 193;
  three runs after the blocks move gave 193, 192, 193. That entry's own caveat said five runs is
  not a measurement, directly below a headline that assumed it was.

## Next

- What the guest branches on. Seven calls separate the reserve from the map, and the trace keeps
  the *tail* of the call sequence for the fault and not the head - so reading them needs an
  ordered record that does not exist yet.
- The rest of the session's turn conclusions, still to be re-derived.
- The clean title's wall, which is platform work with no shim in it.
