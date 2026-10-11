# D787 - A split draw travels as one command

**Status:** decided
**Date:** 2026-10-11

A non-indexed draw that D741 splits into chunks reaches the backend as one command,
`RenderCommand::DrawChunks`: how many chunks, the vertices each full chunk draws, the vertices the
last one draws, the first vertex, and how far each chunk's first vertex lies past the one before.
The driver hands the backend the chunks it names, each as the `Draw` it would have been, so the device
sees what it saw before: one draw-data record and one mesh workgroup per chunk, in one dispatch
while nothing else changes.

What changes is the host's work between the guest's packet and the device. Every pass over a
submission's commands - binding draw buffers and textures, censusing them, counting - meets one
command per guest draw instead of one per chunk. Craft drew about 8,800 chunks a submission,
63 vertices each, and those passes, about a microsecond a chunk, were most of its frame on the
host.

An indexed draw's chunks still travel separately: each reads its own range of the guest's index
buffer, bound before it.

**Why:** the user chose it on 2026-10-11 over leaving D741's chunks as separate draws, once the
per-chunk cost was measured (`ORBISTOUN_PERF_DETAIL`: 264,000 chunks a second in Craft, with
the draw-buffer and texture passes at about 300 ms of each second).

**Rejected:**
- One host draw per chunk, as D741 recorded them: correct, and the cost above.
- One draw-data record per guest draw, with each workgroup finding its chunk from its id: fewer
  records still, but it changes the translated module, and the records were not where the time
  went.
