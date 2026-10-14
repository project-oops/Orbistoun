# D877 - An AMPR command buffer's reset rewinds its commands

**Status:** decided
**Known by:** measured
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

The console does the same (obSCEne `REQ-20261014T1710Z-ar01`,
`reports/hardware/report-20261014-batch9.log`, section `000-boot/ampr-write-address`): after two
write-address commands, +4 0x18 and +8 2, Reset answers 0 and leaves +4 and +8 zero, the size and
buffer kept and the buffer's bytes untouched (`ar01-reset-after-2`); the next command lands at the
buffer's start, +4 0xc and +8 1 (`ar01-append-after-reset`). The same request measured an append
that does not fit: refused with `0x80020010`, the object and buffer unchanged
(`ar01-overflow-append-3`), which is what orbistoun answers.

**Rejected:**
- Leaving the buffer as it is and answering appends past its size: the hardware's answer to an
  append that does not fit is not measured either, and a title that resets before reusing a buffer
  means it to start again.
