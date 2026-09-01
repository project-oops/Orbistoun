# 2026-08-19 - Crunch continued: worker mode and image placement


**170 tests, gate green.** D057 and D058 added; 22 crates.

**Worker mode done** (D057). `orbistoun-worker` holds both halves; the binary
re-invokes itself with a hidden `--worker` flag. `orbistoun-cli run` drives a real
child process.

**Image placement done** (D058). A container's span is reserved and every loadable
segment copied in, with `.bss` zeroed. The **96 MB commercial executable now loads**:
70.5 MB copied, 32.7 MB zeroed, at `0x400000000000`, inside a worker process.

**Surprises.**
- **Host allocation granularity is not the guest page size.** D054 found Windows
  *reserves* at 64 KiB; placement found the sharper edge - a reservation **base** must
  be granularity-aligned too. A span rounded to 4 KiB gets silently rounded down by
  `VirtualAlloc`, which this code then correctly refuses as relocation. The two values
  coincide on Unix, so code written and tested only there would never notice. Now
  queried via `allocation_granularity()` rather than assumed.
- **Surveying imports must not gate placement.** A container with no dynamic table is
  legitimate - a static binary imports nothing - and refusing to load one conflated
  "cannot read this file" with "this file needs nothing". Parsing is the gate;
  surveying is not.
- **A missing file is `Failed`, not `Terminated`.** Two tests asserted the old,
  worse behaviour and had to be corrected: `Failed` means the request was wrong,
  `Terminated` means a guest was loaded and then stopped. Collapsing them makes "the
  path was a typo" and "the emulator cannot go further" indistinguishable to anything
  reading the stream.
- **Two of my own test expectations were wrong again**, both times because the code
  had become *more* correct than the test. Worth noticing as a pattern: when a test
  fails after a behaviour change, check which one is right before fixing the code.

**Next.** Relocation, TLS, and the entry jump - the remaining three pieces of phase 4.
Every prerequisite now exists and is verified: placement works, the call convention is
proven (D056), and the worker gives them a process to happen in.

