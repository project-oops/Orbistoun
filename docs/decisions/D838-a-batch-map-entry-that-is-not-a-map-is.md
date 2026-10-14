# D838 - A batch-map entry that is not a map is refused by name

**Status:** decided
**Date:** 2026-10-14

`sceKernelBatchMap` reads each 32-byte entry's four bytes at +28 as its operation. Zero is a map
of direct memory, which is modelled. Any other value is refused: the entries before it are carried
out and counted in `completed`, it and those after it are not, the call answers the unimplemented
placeholder, and the run names the entry, its range and the operation.

**Why:** the field was read as unused flags, so every entry was mapped whatever it asked for.
Grand Theft Auto V batches operation 1 at a range it mapped before, alongside maps of the same
address. Carried out as a map it placed memory the title asked to change, and a later map of a
longer range at that address conflicted with it. A refusal says what is not modelled; a map in
its place answers success for work that was not done.

**Open:** what operations other than 0 do is unmeasured. obSCEne's probe and oops-sdk only ever
map. REQ-20261014T0230Z-bm01 asks the hardware what operations 1 to 4 do to a mapped range.
Grand Theft Auto V reads BACK under this until they are modelled.

**Rejected:**
- Treating operation 1 as an unmap on the strength of its position: the value's meaning is not
  measured, and a guess here moves memory the title owns.
- Ignoring the field, as before: a stub answering success.
