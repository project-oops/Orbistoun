# D821 - Submissions may run on the command processor's own thread

**Status:** decided
**Date:** 2026-10-12

Amends D712. A submission is still prepared on the guest's thread, in order: its stream is read,
its shaders found and translated, and the registers it leaves carried to the next. Under
`ORBISTOUN_SUBMIT=async` it is then queued to a command-processor thread, and the submit
returns. That thread carries it out exactly as a synchronous submit does: memory work, draws at
submit, flip releases, and the fences and labels written only for work that ran (D705). Up to
four submissions wait in the queue. A guest further ahead waits at its next submit, timed as the
`queue full` span. A run's ending waits up to two seconds for the queue to drain, so its record
and last frame are the run's whole.

Synchronous stays the default. The window sets `async` for the runs it plays; a command-line
run, a `compat record` and a verdict between two runs keep the order in which two runs agree on
when every fence was written.

The registers a submission leaves are now carried before it is carried out, in both modes, not
after. The next submission is prepared from them, and an asynchronous one may be prepared before
the last has run. Their indirect loads read the tables as submitted, which this order matches.

**Why:** with the guest waiting on every submission's device work, CPU and GPU time added
instead of overlapping; hardware queues a buffer and runs it alongside the CPU. PPSA02664 makes
2,697 flips in 30 s asynchronously against 2,456 synchronously, presenting in both. GLCB00001's
frames match flip for flip, apart from a 15 by 7 pixel box at (232, 100) where an on-screen
number differs; two synchronous runs match byte for byte. The fence rule is unchanged: a guest
waiting on a label waits on work that ran, which is what hardware gives it.

**Rejected:**
- Writing fences at submit and drawing later: tells the guest its pixels exist (D712's own
  rejection, unchanged).
- Asynchronous by default everywhere: a verdict compares two runs, and two asynchronous runs need
  not agree on when a fence was written.
- Moving preparation to the queue as well: the register carry orders preparation, and a guest
  thread waiting on its own submission's translation is the cost the queue exists to hide later,
  not now.
