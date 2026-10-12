# D819 - Audio output plays on a host device through a sink

**Status:** decided
**Date:** 2026-10-12

`orbistoun-audio` hands each chunk a guest's output port plays to a **sink**, at guest
semantics: the port, its channel count, and 16-bit samples at 48 kHz. The sink answers how long
the output call waits. `orbistoun-audio-host` is the one sink: every port has a queue, and the
device's callback mixes the queues into whatever rate and channel layout the device runs at.
The worker installs it when `ORBISTOUN_AUDIO=host`, which the window sets for the runs it plays.
With no sink installed, a port paces itself by its sample rate, as before.

The output call waits until its port holds no more than about 64 ms, never longer than two
chunks. The guest is then paced by the device's clock. Pacing by a sleep while a device drained
the queue would let the two clocks drift apart, and on a host whose timer ticks every 15.6 ms a
sleep overshoots badly enough to empty the queue. The logical clock still advances by the
samples' duration, so a guest timing its output sees the same time either way.

Only selectors 0 and 1 are played. Their format, 16-bit signed interleaved, is `assumed` from
oops-sdk: its `audio.c` opens with selector 1 as "16-bit signed interleaved stereo" and plays on
hardware. Selector 2 opens two channels as measured, but its format is unmeasured, so its
samples reach no sink. `sceAudioOutOutputs` and `sceAudioOut2ContextPush` reach none either; the
run report names output that went unheard, and output that played nowhere because the device
did not open.

**Why:** a title with audio was silent, and nothing in a run said so. The seam keeps the host
audio library out of `orbistoun-audio`, as `orbistoun-gpu` stays clear of `ash`. A second
backend is then another sink, and the port's contract is tested with a sink that records what
it is handed. A command-line run stays silent unless asked: the loop runs titles unattended, and
many at once.

**Rejected:**
- Keeping the sleep and pushing samples alongside it: the queue drifts empty or full with
  the host's timer, and the guest's pacing stops meaning anything about the device.
- A device opened by `orbistoun-audio` itself: it ties the guest-facing crate to one host
  library, and its tests to a host with a sound card.
- Linux and macOS in the same change: `cpal` links the system's ALSA library on Linux, a build
  dependency every Unix image would carry. It is Windows-only, as host gamepads are.
