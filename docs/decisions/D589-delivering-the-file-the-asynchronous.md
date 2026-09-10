# D589 - Delivering the file the asynchronous path resolved changes nothing

**Status:** measured
**Date:** 2026-09-08

## The experiment, and why it was worth running

D587 established what PPSA03416 asks the asynchronous file path for -
`/app0/Media/globalgamemanagers`, one command buffer, one submit, one wait - and that answering
all three with success changes nothing. The reading that fit was that the guest wants **bytes**
rather than a status: the Apr read fails, the title falls back to
`LocalFileSystemPS5::Enumerate` (which prints its own `TODO:` marker), the enumeration finds
nothing, and a singleton is never constructed - which is the null the run dies on.

That reading is testable. `ORBISTOUN_APR_DELIVER` reads the resolved file into the buffer the
command header names, and the guest grades it.

```text
orbistoun: delivered 224748 byte(s) of /app0/Media/globalgamemanagers into 0x740009200000
```

## It does not move

| | distinct imports |
|---|---|
| baseline | 193, 193, 193 |
| delivering | 192, 192, 193 |

**Three runs each, because one is not a measurement.** The first delivering run reported
`FURTHER`, and it was the 192-to-193 drift `Step::CheckRepeats` already measures (D583) landing
the convenient way round - the same trap this session has now fallen into twice and caught twice.
The fault is identical, byte for byte, in every run.

So the guest does not consume the bytes. Either the buffer is not where it expects them, or the
wait has to signal completion some other way - `libSceAmpr` exports
`sceAmprCommandBufferWriteKernelEventQueueOnCompletion`, which suggests completion is reported
through an event queue rather than a return - or the whole reading is wrong.

**This does not withdraw D587**, which established what is asked. It withdraws the *inference*
that supplying it is what the title is waiting for.

## What the experiment is worth anyway

**It converts a guess into a closed question.** Before it, "the guest needs the bytes" was a
sentence in a worklog that would have been repeated until somebody tried it. It is now a
measurement, and the next reading has to explain why the bytes did not help.

The apparatus stays because it is where a corrected reading gets tested: a different buffer, a
different completion signal, a different file all reuse it.

## Two things it had to be built honestly to say

**It intervenes, and the report says so.** Declared `Effect::Intervenes`, and *also* carried in
`Experiments` - the second half is the one that gets forgotten. A diagnostic the conditions
record does not know about produces a verdict with no caveat beside it, which is the entire
failure `needs_caveat` exists to prevent (D569). Without it the run printed `same` with nothing
saying the run was propped.

**The reader is installed from above.** These are file operations under the `libkernel` name, and
`orbistoun-kernel` and `orbistoun-fs` are sibling subsystems - the relation that kept the clocks
out of `orbistoun-libc` and put them in `orbistoun-hle` (D536). Splitting `libkernel` across two
crates is the alternative, and a library with two owners is a new concept rather than a new
function. So `orbistoun-worker`, which owns both, installs the reader, and the kernel calls
through a hook - the inversion `on_guest_stop` already uses (D160).

## What this does not establish

**That the buffer is not the destination.** It establishes that filling it does not change what
the guest does. A guest that never looks and a guest that looks and rejects are the same result
here.

**Nor that the file is the right one.** The path comes from the last resolve call, because the
command storage is empty and names nothing. A title resolving several would get whichever was
last, and this one resolves exactly one.

**Nor anything about `LocalFileSystemPS5::Enumerate`.** The fallback story is still the reading
that fits and is still unmeasured; what this rules out is the step before it.
