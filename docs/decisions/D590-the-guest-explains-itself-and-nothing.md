# D590 - The guest explains itself, and nothing was reading it

**Status:** measured
**Date:** 2026-09-08

## Four sessions of inference, and the answer was a string orbistoun rendered

PPSA03416's wall has been chased through a null singleton (D576), a crash reporter (worklog 426),
an asynchronous file path (D587) and a delivery experiment that failed (D589). Every step was an
inference from what the guest *did*.

The guest had been saying it in words the whole time:

```text
submitCommandBufferAndGetResult error=2147418113
waitCommandBufferCompletion error=2147418113
Unknown error occurred while loading '/app0/Media/globalgamemanagers'.
Failed to load PlayerSettings (internal index #0).
Most likely data file is corrupted, or built with mismatching
editor and platform support versions.
```

**orbistoun rendered every one of those strings and threw them away.** `render_with` is the one
function every format entry point reaches; it produced the text, handed it to the guest, and
kept nothing.

Some of it reaches a stream and is visible - the two `error=` lines are, which is how D587 found
the asynchronous file path at all. The rest is formatted into a buffer the guest passes to its
own logger, and a logger that ends at `sceKernelDebugOutText` or at a service nothing implements
is a message nobody sees. **The three lines that name the actual failure are in the second
group.**

## What they establish

The chain is no longer inferred. The Apr submit and wait fail with orbistoun's placeholder; the
guest cannot load `globalgamemanagers`; `PlayerSettings` is the first object in that file and
does not load; the title stops. Everything after that - the abnormal-termination report, the null
singleton, the fault - is the guest dying tidily.

It also **corrects D589's framing**. That entry concluded the Apr path was not what the title was
waiting for, because delivering the bytes changed nothing. The guest's own words say it is
exactly what it was waiting for; what D589 measured is that delivering bytes *to that address*
is not how the guest collects them.

And running both interventions together - forced success **and** delivery - removes the two
`error=` lines and leaves the load failure. Which is the two-dimensional case D283 and D286
already named, run correctly at last, and still negative: the return and the buffer are not the
two dimensions that matter.

## Recorded, gated, and printed after the guest stops

`ORBISTOUN_TRACE_FORMAT` keeps what `render_with` produced. Off by default, because a title
formats constantly - 125 strings in a boot that walls early, and far more in one that runs. On
the guest's own stack it takes a lock and pushes a string and nothing else; the printing happens
after the guest has stopped (D381, principle 9).

**The last strings rather than the first**, which is the opposite of what `opened` keeps. A path
is a path whenever it appears; a guest says the interesting thing immediately before it stops, so
a record that filled early and then refused would keep the least useful end of the run.

## What this does not establish

**Where the guest expects the file's bytes.** It establishes that the guest tried, failed, and
said so. The two `error=` lines can be silenced and the load still fails, so the reading in D589
- that the buffer at the header's `+0x10` is the destination - is not supported by anything.

**Nor that this is every message.** It records what `render_with` renders. A guest writing bytes
it assembled itself, or calling a formatter orbistoun does not implement, says nothing here - and
`vsnprintf` was the most-imported unimplemented name in this project for a reason.

**Nor that the strings are trustworthy as diagnosis.** They are the *title's* account of its own
failure, which is evidence about what the title believes. "Most likely data file is corrupted" is
Unity's generic message for a header it could not parse, and it is wrong about the cause here -
the file is intact and was never delivered.
