# D672 - The audio drain was measurable without a device

**Status:** decided
**Date:** 2026-09-10

## What was holding the port calls back

`orbistoun-audio` served `Init` and `Close` and nothing else. The reason is in its own header
and it is a good one: *"guests frequently block on audio-buffer completion, so a stub that never
signals a drained buffer hangs the title with no audio symptom to point at it"* (D171). There is
no audio backend, so a port that opened could never drain, and a title waiting on one would sit
there forever with silence as the only clue.

So `Close` answered `0x8026_0003` to every handle, because no handle could name a port.

## The measurement that changed it

obSCEne measured the whole contract on a title leg, and the load-bearing part is that
**output blocks and for how long**:

| check | what a console did |
|---|---|
| `090-audio/open-shapes` | 48 kHz accepted at chunk `0x100`, `0x200`, `0x400`, `0x800`; 44.1 kHz refused at all four with `0x8026_0008` |
| `090-audio/format-selector` | port state is 16 bytes; selector 0 → `81 00 **01** c7 ff ff 05 00`, selectors 1 and 2 → the same with `02` |
| `090-audio/blocking` | eight 512-frame buffers took `0xdedc` µs - 57 ms |
| `090-audio/volume-flag` | flags 1, 2 and 3 each answered `0x0` |

The drain does not need a device. It is arithmetic on the sample rate, and the timing check is
what says so rather than a header.

## What is transcribed and what is decided

**The format split is on the frequency alone.** Chunk size decided no measured case - all four
were accepted at 48 kHz and all four refused at 44.1 - so the shim decides on nothing else. A
version that also refused an unmeasured chunk size would be inventing a rule the data does not
contain.

**The port state is bytes, not a struct**, for the reason D671 gives for the pad: offset two is
the channel count because obSCEne reads it as one and the three selectors differ there and
nowhere else. What `0x81`, `0xc7`, `0xffff` and `0x05` mean is not claimed.

**Output waits the full duration, with no queue.** The console returned in 57 ms against 85 ms of
audio, which is a queue a few buffers deep absorbing the first calls. How deep is not measured.
Picking a number would be inventing the single thing this could get wrong, so every call waits
and the total lands above the check's 40 ms floor rather than near it.

**The return is assumed.** obSCEne discards it, so nothing has seen what a console answers. The
frame count is the conventional shape and a guest testing `rc < 0` reads it as success either way.

## The clock had to agree

First run with all of it in place: `090-audio/blocking` still reported **1 µs** and read as not
blocking - while `sceAudioOutOutput` was returning `0x200`, so the shim was plainly running and
plainly sleeping.

The guest's clock is **logical by default**: it advances a microsecond per read so a run repeats,
because a measurement that cannot be repeated is not one (D181, D238, D582). Eight buffers that
really slept 85 ms read as 8 µs to the guest.

`clocks::advance` exists for exactly this and says so in its own doc - *"A sleep is time passing,
and the clock has to agree"* - added when `usleep` was found sleeping without it. The same wiring
hazard, the same fix, one subsystem over: **every call that makes time pass has to say so.**

Worth recording as a rule rather than as an incident, because it will happen again the next time
something blocks: a real sleep is invisible to the guest unless the clock is told.

## What it bought

Every audio check passes - eight of eight, including two that had never run. `090-audio/blocking`
reports `0x14d51`, 85,329 µs, which is eight 512-frame buffers at 48 kHz to the microsecond.

Against the matching hardware leg, the checks where hardware passes and orbistoun does not went
from **ten to five**, and orbistoun now passes **174** against hardware's 167. What is left is
`080-video/visual-flip` and `111-modlink/walk` (both skipped for capability orbistoun lacks),
`101-input-ext/mouse-read` and `130-layout/system-software-version` (partial), and
`900-surface/control` (the one outright failure).
