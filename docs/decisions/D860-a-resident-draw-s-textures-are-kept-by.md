# D860 - A resident draw's textures are kept by content, apart from its pipeline

**Status:** measured
**Date:** 2026-10-14

**Amends:** D856 - a pipeline built for new textures shared its device pipeline but still made,
copied in and released its own textures.

A resident draw's sampled texture is kept by what it holds: its texels' hash, taken once at the
bind, with its extent, encoding, faces, slices, levels and sampler. A pipeline built for a draw
binds the kept texture for each slot and makes one only for texels not yet kept. A kept texture is
copied in once, by the first pipeline that binds it, and none after; a pipeline release leaves it
alone. Kept textures are released together, with every cached pipeline whose set points at them,
once they hold 256 MiB, and only before a pipeline is built, after the draws recorded so far have
run.

Resident pipelines share one observation buffer and one storage image, which nothing reads back
after a resident draw. A pipeline whose textures are all kept and copied in therefore sets nothing
up, and its build does not wait on the device.

Words nothing hashed - the harness's draws - and the harness's coarse level are made into a texture
per pipeline, as before.

**Why:** a guest that changes which textures a draw samples changes its pipeline key, and each new
key made every texture again, copied them in through a submission the device was waited on for, and
at 512 pipelines settled and released them all. With the textures kept, `ab` against the commit
before, three alternating 30-second rounds each:

| title | fps before | fps after | median frame ms |
|---|---|---|---|
| NVRB00001 | 5.5 (5.4, 5.1, 6.1) | 7.2 (7.0, 7.2, 7.5) | 110 -> 90 |
| STKT00001 | 1.8 (2.3, 1.1, 2.1) | 4.5 (5.5, 1.2, 6.9) | 196 -> 153 |

`execute hand-off`, a guest thread waiting on the device thread, fell from 22% of the run to 12%
(Neverball) and from 25% to 16% (STK); `release` and `build: resources` all but went.

**Rejected:**
- Sampled images in a set of their own, bound per draw as its buffers are (D733), with the pipeline
  keyed without its textures. It is the end state, but it moves every translated module's sampler
  bindings; keeping the textures takes the cost that was measured without that.
- Releasing a kept texture on its own when it is no longer bound: a cached pipeline's set may still
  point at it, and tracking which would cost more than the drain it saves.
