# D101 - Guest memory is a separate binding

**Status:** decided
**Date:** 2026-09-26

A translated module binds guest memory separately from the observation window its registers
are copied into. A guest access outside the memory window reads zero and drops writes, checked
per word by address.

**Why:** in one shared buffer a guest store could reach the observation window, and a memory
fault would present as a register fault in an unrelated instruction. Out-of-range storage-buffer
access is undefined, and wrapping an overrun onto word zero turns an ordinary guest bug into a
plausible corruption of the start of memory.

**Rejected:**
- One buffer with memory at an offset: guest stores can reach the registers.
- Masking or wrapping the index: an overrun lands somewhere the guest never asked for.
