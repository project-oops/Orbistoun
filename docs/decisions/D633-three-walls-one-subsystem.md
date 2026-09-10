# D633 - Three walls, one subsystem

**Status:** measured
**Date:** 2026-09-08

## What a day of narrowing converged on

Three findings this session arrived from unrelated directions and landed in the same place. Each
is recorded on its own; this exists because **they are one decision, not three**, and a reader
meeting them separately would not see that.

### The title's own code, mapped and stubbed (D630)

`PS5Util.prx` and `Il2cppUserAssemblies.prx` are files PPSA25872 ships. Orbistoun places,
relocates and starts both. **Six of the executable's imports come from them, all six are in those
modules' export tables, and all six are answered with the `Unimplemented` placeholder.** One of
them is then called 19,689,015 times - 87.6% of every call this project has recorded - by a guest
that asked, was told nothing, and asked again until its budget ran out.

`LinkedTitle::exports()` already indexes exactly what a fix would need, by library and by hash.
What is missing is the step that consults it.

### A symbol that should have been bound, left null (D628)

obSCEne's payload, entered with the handoff argument, dies at `obs_sink_open+0x73` calling `0`.
`sceKernelGettimeofday` **has a stub slot** - `ORBISTOUN_DUMP` arms it and captures nothing - so
the guest is not reaching the stub, it is calling zero. A stub exists and nothing points at it.

### A symbol that should be null, bound (this record)

`900-surface/control` is obSCEne checking its own instruments: `obs_census_control_present` is a
weak symbol it defines, `obs_census_control_absent` one it deliberately never defines. The console
reports the first present and the second absent. **Orbistoun reports both present**, so the probe
declares its own census meaningless - which is why ninety-four census entries carry a warning
rather than a reading.

Neither symbol is an import - they do not appear in orbistoun's import list at all - so this is
the guest's own link, resolved differently here. The mechanism is relocation-shaped and **which
relocation has not been established**, so it is stated at that strength.

## The shape

A binding that should exist and does not; a binding that should not and does; and an import
resolved to a stub when the module providing it is mapped. Three symptoms, one question: **what
orbistoun writes into a guest's relocation slots.**

Nothing here needs a hardware measurement, a name, a vocabulary or a new capability. Each is
diagnosed to a file, a function and an offset. What each needs is a change to import resolution,
which this session's brief places outside the work - *"strictly confined to user-space standard
library implementations (libc, pthreads, memory management, and ABI call stubs)"*.

So they are written down together. The largest wall measured anywhere in this corpus is in that
subsystem, and so is the reason its most valuable guest cannot report on itself.

## What was done instead, and what it is worth

Everything reachable without touching it, and the differential is the measure: **17 distinct
findings down to 13**, with four *"library loaded but no symbols resolved"* entries retired by
making by-name resolution agree with by-import resolution (D632), seven more identified as the
probe declining to call rather than orbistoun refusing (D626), and ninety-nine repetitions of one
sentence stopped from burying the sixteen that each said something (D624).

None of that moved a title's wall. All of it moved what a run can be *read* to mean, which is the
thing that was blocking the reading.
