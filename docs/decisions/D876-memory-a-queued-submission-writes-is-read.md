# D876 - Memory a queued submission writes is read once it has been carried out

**Status:** decided
**Date:** 2026-10-14

Amends D874. What a preparation read waits for is everything a queued submission writes, not only
the colour targets it draws into: the command processor's own `WRITE_DATA` to memory, `DMA_DATA`
fills and copies and value-storing `RELEASE_MEM` (`cp::written_spans`, from the packet walk), and
each compute dispatch's buffers and window (`Pipeline::dispatch_write_spans`, from its state and
bindings, without translating or running it). The queue keeps these spans beside the drawn targets.
A submission whose writes cannot be bounded before it runs - a dispatch whose state the registers
do not give, whose program does not decode or trace, or that stores other than through a buffer
(an image, a global or flat address), or a draw that streams out (D861) - is queued as writing
everything, and every preparation read waits for it.

The spans are worked out on the guest's thread as the submission is queued, and only
asynchronously: each dispatch's state and whether any draw streams out ride on the `Submission`
from its preparation, and the dispatch's program is decoded and its buffers traced again there.

A read inside a preparation no longer waits where it is. The preparation holds the live pipeline,
and the command processor carries a dispatch or a run of draws out through that same pipeline, so
the wait D874 put in the read could only time out, ten seconds later, and read what it would have
read anyway. The read is noted instead; the preparation finishes, the pipeline is released, the
queued submissions up to the latest one that writes over a read are carried out - those queued
after it are not waited for - and the submission is prepared again
(`command_processor::preparing`), up to three times, since another guest thread can queue in
between. Preparing a stream again gives the same
submission: its registers come from the state carried in, which only `carry_registers` advances,
after the preparation that is kept.

Preparation stays on the guest's thread: moving it to the command processor's was measured about
20% slower (D874).

**Why:** a later submission's preparation reads texels, descriptor tables, buffers and shader code
through `SettledReads`, and the command processor's thread is the only one that carries out a
queued submission's packets and dispatches. Prepared ahead of them, a texture a dispatch stores to,
a buffer a `DMA_DATA` copies into, or a table a `WRITE_DATA` patches was read as it stood before.
The test `a_preparation_reading_what_a_queued_submission_writes_is_made_again_after_it` holds the
processor off and checks that a preparation reading a queued write is made again, and returns only
once the write has been carried out; `a_stream_writes_where_its_packets_land` checks the spans.

**Not covered:** a depth or stencil target a queued submission draws into, and memory written by
packets the command processor here does not carry out (they stop the queue, D710), are still read
as they stood before the submission. The spans a dispatch writes include every byte of its buffers
and window, read or stored, so a read of a buffer a queued dispatch only reads also waits.
