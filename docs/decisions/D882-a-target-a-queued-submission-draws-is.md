# D882 - A target a queued submission draws is sampled as it is carried out

**Status:** decided
**Date:** 2026-10-14

Amends D874 and D876. Asynchronously (D821), a preparation on the guest's thread that reads, for
its own draws, memory a queued submission draws into or writes - a colour target, a buffer a
dispatch stores to, a range its packets write, or the frame pending while anything is queued -
no longer waits for that submission. The read is noted (`command_processor::preparing` answers
it beside the preparation), the guest runs on, and the submission is marked
`Submission::read_ahead_of_draws`. When the command processor carries it out, after the
submissions ahead of it, it draws even a segment holding all its draws as it draws any other
segment (D729): prepared again from the stream through the live pipeline, its reads made on that
thread, where the earlier draws and dispatches are done and the pending frame is written back
before a read over it (D714, D719).

What a preparation carries forward past its own draws is still read after the queue, on the
guest's thread: the tables its registers load from (`Submission::tables`, checked by
`TableSnapshot::spans`), and the bindings its queued writes are worked out from (D876). Those are
read inside `command_processor::carrying`; a read there over a queued write or draw makes the
preparation again once the queue is carried out that far, as D876 made every one. Preparation
itself stays on the guest's thread (D874): only the draws of a submission that read ahead are
prepared a second time, on the command processor's.

A dispatch's queued writes are narrowed to what it stores to: each buffer one of its buffer
stores or atomics goes through, and its window only where one stores through that. A buffer it
only reads is not written.

**Why:** PPSA28061 samples, every frame, what the submission queued just before it renders, and
a 384 KiB buffer a queued dispatch stores to. With D876 every one of those preparations waited
for the queue and was made again: 1,600 a 30 s run over drawn targets, then 290-350 over the
dispatch's buffer once those ran on, the guest idle for 17-29% of the run (the `prepare again`
span). The hardware reads them after the draws and dispatches; drawing the submission's draws
again on the command processor's thread reads them there, in that order, without holding the
guest. The test `a_preparation_sampling_what_a_queued_submission_draws_runs_on_and_is_drawn_again`
holds the processor off and checks that such a preparation, over a drawn target or a stored
buffer, is made once and returns with the submissions still queued, answered as read ahead;
`a_preparation_reading_what_a_queued_submission_writes_is_made_again_after_it` checks a read of
carried-forward state is still made again after them; `a_dispatch_binds_buffers_no_window_spans`
checks a copy's write spans are its destination alone.

**Not covered:** a guest-thread preparation reads shader code and descriptors for its own draws
as they stood before the queued writes, and so translates and caches what it found there; the
draws prepared again on the command processor's thread translate what is there then, and the
stale translation stays cached under its own content.
