# D877 - An AMPR command buffer's reset rewinds its commands

**Status:** decided
**Known by:** guest-observed
**Date:** 2026-10-14

`sceAmprCommandBufferReset(obj)` rewinds a command buffer that has commands in it: the bytes used
(+4) and the command count (+8) go to zero, the bound buffer and its size are kept, and the next
command is written from the buffer's start. An object that cannot be written answers the
placeholder.

**Why:** Grand Theft Auto V resets a command buffer it filled with read-file and write-address
commands, then appends to it again. Unrewound, the append did not fit, was answered with the
placeholder, and the title trapped (`int 0x41`) on the answer. Rewound, it runs to its time limit
38 million calls further, and no other title moves back: PPSA28061 and PPSA21564, the other titles
that use AMPR, were run beside it (D832's corpus check).

Reset was measured only on a freshly bound buffer, where the counters are already zero and nothing
changes (obSCEne REQ a5b1). What it does to a buffer with commands in it is not measured:
obSCEne `REQ-20261014T1710Z-ar01` asks, and this decision is retired or kept by its rows.

**Rejected:**
- Leaving the buffer as it is and answering appends past its size: the hardware's answer to an
  append that does not fit is not measured either, and a title that resets before reusing a buffer
  means it to start again.
