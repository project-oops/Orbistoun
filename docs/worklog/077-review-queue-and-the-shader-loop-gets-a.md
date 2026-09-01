# Review queue, and the shader loop gets a verdict


Five decisions closed with input - D086 (ranking), D106 (Auto warns), D104 (`exp` stays
refused, with the way out written up as G11), D101 (binding split confirmed, wrapping
fixed), D113 (the cache verifies on a hit). The queue is down from 19 to 13.

Then D148: `orbistoun-cli shaders` reports `FURTHER` / `same` / `BACK` against the previous
run, in the import side's vocabulary. 640 workspace tests green.

### Surprises

- **The cache verification found a bug in itself on the first run.** Reading a word past
  the end of a short shader answered zero, so any two shaders shorter than a word compared
  equal - the exact "two shaders satisfy one entry" fault the check exists to prevent, at
  the one end nobody thinks about.
- **The memory window was aliasing, not clamping.** `word_index` masks, and masking a word
  index past the end lands it on word *zero*. A guest overrunning a buffer is an ordinary
  bug; a translator that turns the overrun into a plausible corruption of the start of
  memory makes that bug unrecognisable. Reads answer zero and writes are dropped now, and
  multi-word accesses step by address so a tail crossing the boundary is caught per word.
- **A draft test wanted a hole in a production type.** It needed a way to plant a cache
  entry, which would have meant a test-only method on `Pipeline`. The branch was reachable
  honestly as a unit test on the entry type itself; the shortcut would have widened the
  API to reach it.

### Outstanding

The GPU loop's other two rungs are not built: submissions do not contribute to a run's
progress block, and address provenance - matching an address in a submission against the
allocations the shim handed out - does not exist. Both are the measurement side of the
GPU-VA question (D101) and neither needs a capture to build.

