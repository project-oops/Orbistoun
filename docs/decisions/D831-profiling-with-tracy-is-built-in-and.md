# D831 - Profiling with Tracy is built in and asked for at run time

**Status:** assumed
**Date:** 2026-10-14

`orbistoun-cli` and `orbistoun-gui` build with the `tracy` feature by default (D829), and the
client stays dormant: a worker starts it only when `ORBISTOUN_TRACY=1`, on `TRACY_PORT`, and no
port is opened otherwise. Each guest flip also plots its frame time.

Around it, `orbistoun-perf` holds what every front end shares: finding the Tracy tools
(`ORBISTOUN_TRACY_TOOLS`, the copy installed under the data root, then `PATH`), installing the
pinned version with the host's own `curl` and `tar`, recording a run headless with
`tracy-capture` on a free port, and reading the recording back with `tracy-csvexport` as zones
ranked by self time and by total time, with the frame times' percentiles. Two commands use it:

- `orbistoun-cli perf <title>` records one run into the title's `perf` directory and prints the
  report beside it. It writes nothing into the title's compat record: a profiled run measures the
  emulator's time, not how far the title got.
- `orbistoun-cli bench` records every title in the library in turn - with its compat record's pad
  script, homebrew on the host clock - skips a title while something else keeps the GPU busy,
  keeps the results under the data root, and ranks the zones across titles with each title
  weighed equally, beside the last benchmark's frame rates.

**Why:** a profiler that has to be built for is not used while a cost is fresh, and a user who
wants to say why a title is slow cannot rebuild. Built in, it costs a span one check whether a
client runs: three alternating pairs of runs of MDEM00001 and PPSA25872, plain build against
dormant client, differed by less than the runs differed among themselves (MDEM00001 173 against
182 fps on average; PPSA25872 148/158 against 150/161 where no other run shared the machine).
Ranking by self time names what to make faster; percentiles of frame time name what a player
feels, which an average hides - the first `perf` of MDEM00001 showed a 3.3 ms median frame beside
a 308 ms p99 of pipeline builds. Weighing each title equally keeps one long-running title from
deciding the order of every other's costs.

**Rejected:**
- D829's opt-in build: measured, the dormant client costs nothing a benchmark can see, and a
  build nobody has is a profiler nobody uses.
- Recording a profiled run in the compat record: it would rewrite tracked records with runs taken
  for a different question.
- A fixed port: two runs profiled at once, as concurrent sessions do here, would both reach for
  8086.
