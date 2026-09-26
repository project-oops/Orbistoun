# D209 - Table generators replay a committed recording

**Status:** decided
**Date:** 2026-08-24

`orbistoun-gen` reaches the reference assembler through `assembler::Source`, which is either a
live assembler or a replay of a committed recording. The check gate regenerates the committed
tables from the replay and fails on any difference. A recording carries its inputs, keyed by
probe text, and a run that produces nothing fails.

**Why:** the assembler build the solvers need is not available on most machines, and a generator
runnable on one machine leaves nothing to catch a hand-edited table. Keying by call order hands
every probe the same canned answer on replay. An empty table exiting zero deletes the reference
output unnoticed.

**Rejected:**
- Live-only generation: nothing checks committed tables.
- A cache that decides its own staleness: a recording is a decision somebody made.
- Overwriting the annotated encoding table: its reasoning and citations are prose.
