# D710 - Command-processor memory packets execute at submit

**Status:** assumed
**Date:** 2026-09-24

At submit, the command processor's own memory work runs on the CPU in stream order: data fills
and copies, end-of-pipe writes and memory waits; register writes, no-ops and cache control pass
with no effect. Any other packet stops execution and nothing after it retires, and writes go
only to ranges the kernel's live tables mark wholly writable.

**Why:** a fill is a fill whoever performs it, so running it meets the rule that a fence stands
for work that ran (D705). The stop rule keeps any fence after unexecuted work unwritten, and a
memory wait that does not hold stops, since nothing runs concurrently to satisfy it. The clock
counter's rate and the absence of observable cache effects are assumed.

**Rejected:**
- Writing the fence without running the packets: claims work not performed.
- Skipping packets that need the GPU and continuing: retires fences after work that did not run.
