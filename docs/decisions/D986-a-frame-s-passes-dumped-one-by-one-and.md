# D986 - A frame's passes dumped one by one, and their outputs judged

**Status:** decided
**Date:** 2026-10-26
**known_by:** measured

`ORBISTOUN_PASS_DUMP=1` dumps every pass of each frame `ORBISTOUN_FRAME_AT` names, `busy:N`
included, into `passes-<flip>/` where the frames go. A pass is one compute dispatch, or one run of
draws into the same colour targets. Consecutive draws into the same targets extend one pass, and
the targets are read back only once something else comes next. After each pass, the images it
wrote are written out: a dispatch's stored images as the device answered them, and a run of draws'
colour targets as read back. Two listings go beside them:

- `passes.tsv`: each pass's number, kind, guest program (a dispatch's address, or the draws'
  pixel programs), work, and the guest addresses of the images it read;
- `outputs.tsv`: each image a pass wrote, with its address, format, extent and file.

An image a pass left as it found it is listed by the earlier file. A pass the host refused is
listed with what it would have written, where its preparation got that far.

`orbistoun-cli passes <dir>` turns a dump into one picture an image, `passes.html`, and a table.
The table flags each pass whose output:

- holds NaN or infinity;
- is nothing but zero;
- is noise: neighbouring texels differ, on average, more than 0.6 of what texels half the image
  apart differ by;
- was refused;
- reads an image a refused pass was to write (`stale`);
- changed nothing.

It names the first pass whose output is non-finite or noise.

A cache of guest memory checked only against host writes must first settle the range it covers,
as a read of it would. Settling covers a frame still on the device, a copy or fill deferred onto
its pages, and a queued submission's draws over it. `GuestMemory::settle` does this without
reading: it is a no-op by default, and the command processor's reader implements it. Texture
binds (tiled, linear and mip chains) and draw buffers call it before they consult their caches.

**Why:** Grand Theft Auto V's in-world frame showed coloured static. The pass dump of flip 5349
found the first non-finite output in the lighting pass (311), which read the reflection chain at
`0x7400e82c0000`. The chain is drawn one level at a time, each level sampling the one before, and
every level had sampled the chain as the texel cache last held it. That was zeros for the large
levels, and stale contents for the mip tail. The tail level then held NaN, and the lighting spread
that NaN over every surface.

A cache answer skipped the read that would have written the pending frame back. It also skipped
the read that would have carried out the deferred copy, and the read that would have marked the
preparation as read ahead of queued draws (D882). The host's write watch never saw a write, so the
cache stayed valid indefinitely.

**Rejected:**
- Making each cache ask the command processor whether a frame or a deferred copy overlaps it:
  correct for those two cases, but it misses a queued submission's draws, which only the read path
  notes, and it puts the command processor's bookkeeping into every cache.
- Turning the texel cache off for ranges a target ever drew into: this costs a detile per bind
  even when nothing was drawn, and a level drawn once and sampled every frame is the common case.
