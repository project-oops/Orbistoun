# D874 - A texture a queued submission draws into is read once it has been carried out

**Status:** decided
**Date:** 2026-10-14

Amends D821. A submission is still prepared on the guest's thread, ahead of the submissions queued
for the command processor. A preparation read - a texture, a buffer, the shader window - that
overlaps a colour target a queued submission draws into waits for the queue to drain first, and
then settles as D714 and D719 settle it: the frame now pending over the read is written back
before the read returns. The same wait comes before a read that overlaps the frame already
pending while anything is queued, since a queued submission drawing into another target writes
that frame back on the command processor's thread.

What a submission draws into is `Submission::drawn_targets`: colour target zero of each run of its
draws (`colour_target_runs`), from the registers in force at the run, found as the submission is
prepared. The queue keeps each queued submission's list beside it. A read overlapping none of them
does not wait, so the guest still runs on while the command processor works.

The command processor's own thread never waits: what it prepares (D729's runs) it prepares after
every submission ahead of the one it is carrying out.

**Why:** the hardware carries out submission N before N+1 reads. Prepared ahead of N, N+1's
binding read a texture N draws into from before N's draws: D714 writes a frame back only once it
is pending, and a target is pending only once the command processor has drawn it. A render-to-
texture pass submitted ahead of the pass sampling it sampled the previous frame's texels, or the
cleared memory under them on the first. Without the wait, the test
`a_read_while_preparing_waits_for_a_queued_submission_drawing_over_it` holds the processor off and
finds the read returning at once with the drawing submission still queued.

Moving preparation onto the command processor's thread orders every read as well, and was
measured about 20% slower: it gives up the guest/command-processor overlap.

Memory a queued submission writes other than through a drawn colour target - a dispatch's stores,
the command processor's own `WRITE_DATA`, `DMA_DATA` and fills - is waited on the same way (D876),
which also moves the wait out of the read: a preparation that read a queued write is made again
once the queue has drained.
