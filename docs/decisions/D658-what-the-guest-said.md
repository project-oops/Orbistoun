# D658 - What the guest said

**Status:** measured
**Date:** 2026-09-09

## The one signal that arrives already interpreted

Every other thing a run produces has to be reasoned back to a cause: a fault address, an import
count, a thread that stopped calling. Each of the last six walls took a measurement, a request to
another project, or both.

A guest that is failing usually **says so**. Engines log their own boot and write a diagnosis
before they die, and that text needs no interpretation at all - it *is* the cause, in the guest's
own words. D186 used this once, by hand: two titles printed diagnostics through an implemented
`printf` and reading them named four functions. Nothing made it systematic, so nobody looked
again.

## Generic by construction, not by effort

The obvious version of this is an engine-specific log reader, and it would be worthless: written
for one engine, rewritten for the next, and useless on a homebrew payload.

The capture point is the **platform ABI** instead - the C library's format family
(`printf`, `sprintf`, `snprintf`), the console's `sceKernelDebugOutText`, and writes to the
standard descriptors. That is the one thing every guest on this target shares whatever it was
built with. Six call sites, no knowledge of any engine, and a `printf` from an open-toolchain
payload lands in the same ring as a commercial engine's own logger.

**It captures what the guest formatted *or* wrote**, deliberately broader than what it printed. A
message formatted into a buffer and handed to a write path this project does not implement has
still been said - and that is exactly the case worth seeing, because the message survives even
when its channel does not.

## Bounded and allocation-free

A fixed 64 KiB byte ring, written under a lock, read once the guest has stopped. Recording must
not allocate (principle 9), and an unbounded log is a log that changes the program it observes: a
guest printing in a loop would take the process down before the run ended. The ring keeps the
**most recent** bytes, because a guest describes its problem immediately before it stops.

One display decision, stated as one: an utterance that does not end in a newline gets one, because
a `printf` without a trailing newline is still a separate thing the guest said. Without it the
first run produced `Argument Count = 1Arg 0 = ...` - two statements read as one, and 121 formatted
messages collapsed into five lines.

## It is not classified, and that is deliberate

It would be easy to hunt for "error" or "fail" and put those lines first. That is a heuristic
dressed as a diagnosis, and this project's rule is that a message naming a cause comes from the
branch that determined it. Nothing here determined anything. The guest's words are shown in the
guest's order, and the reader draws the conclusion.

## What it said immediately

PPSA25872, whose wall four decisions have been about:

```text
[libil2cpp] sceAppContentInitialize returned 0x7fff0001
```

**The engine names the function and quotes back orbistoun's own `Unimplemented` placeholder.** It
then enters the diagnostic dump that `image+0x17554a3` faults inside. `REQ-20260909T2145Z-9e52`
had already asked obSCEne about that function on the reasoning that a title unable to initialise
its app content has something to report; this turns that from a ranked guess into the guest
saying it outright.

And PPSA02664, which had never been read at all:

```text
outbuffer 0_8640KB … 1_8640KB … 2_8640KB … 3_2560KB … 4_192KB
todo: sceVideoOutInitializeOutputOptions will be available in a future SDK
path /app0/Media/globalgamemanagers is not considered suitable for apr reads
TODO: virtual bool LocalFileSystemPS5::Enumerate(...)
todo: void GfxDevicePS5SharedData::CreateWorkload()
```

Three 8.6 MB display buffers allocated, video-out options attempted, Unity's own boot asset read,
and the graphics device creating its workload. **This title is further toward a first frame than
the one the last week of walls has been about**, and nothing in a fault address or an import count
said so.

## What changes because of it

The ranked findings answer *what to implement*. This answers *where the guest thinks it is*, which
is a different question and the one that was missing. Both go in the report; neither replaces the
other.
