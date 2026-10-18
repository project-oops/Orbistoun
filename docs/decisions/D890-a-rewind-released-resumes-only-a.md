# D890 - A REWIND released resumes only a command stream held at it

**Status:** decided
**Date:** 2026-10-18

orbistoun carries a submission out from what it holds when submitted, following each
`INDIRECT_BUFFER` it reaches. A `REWIND` not yet released ends the stream where it stands, and the
address of that `REWIND` is kept as held. When the title releases a `REWIND`
(`sceAgcRewindPatchSetRewindState(rewind, 1)`), the stream held at it goes on: what follows it is
carried out, up to and with the jump that leaves its buffer, in order behind what came before. A
`REWIND` released that no stream is held at only has its valid bit set; the stream that reaches it
later passes it.

**Why:** PPSA04263 feeds the GPU as a ring. Each buffer ends in a jump to a 14-dword trampoline
holding a `REWIND` and a jump it fills in once its next buffer is written, and then it releases
the `REWIND`. The console's command processor runs behind the title: it holds at a `REWIND` it has
reached, and passes one already released. 7cd7387c carried a release's continuation out whenever
the title released it, and the title often releases a trampoline's `REWIND` before it submits the
buffer that leads there - so the continuation ran ahead of its predecessor, and the submit then
followed the filled-in jump again. Two boot runs in three stalled at the second flip (0568dfe6
unwired it). Only a stream that reached the `REWIND` can resume from it.

**Rejected:**
- Passing over every jump in a submitted buffer. Deterministic, but the buffers the jumps lead to
  never run: the title's end-of-pipe label stopped short of the prologue's fence.
- A command processor thread that stalls at a `REWIND` and re-reads guest memory. The same order,
  but preparation would have to move off the guest's thread, which costs more than it buys (an
  earlier measurement moved it and lost a fifth of the frame rate).
