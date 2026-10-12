# D812 - content export starts for a title's allocator block

**Status:** decided
**Date:** 2026-10-12

`sceContentExportInit2(param)` answers 0 for a parameter block, writing nothing and calling
neither of the allocation and release functions the block's first two words hold. A null block
answers the placeholder. Nothing else in `libSceContentExport` is implemented.

Guest-observed (`known_by = guest-observed`). The library binds in no probe leg, by link or by
name, on a non-retail eboot (REQ-ce01), so its answer cannot be measured. PPSA21564 calls it once
at start-up with its allocator block, asserts the answer is 0 (`ShareService.cpp` line 102) and
traps with `int 0x41` when it is not. The retail title runs on the console (D709), so the console
answers 0 there.

What the library does with the callbacks is not observed: the title never reads anything back
through them, so none is called.

**Why:** the placeholder is not 0, so PPSA21564 stopped at its own assertion at start-up.

**Rejected:**
- `sceContentExportTerm`: the title ignores what it answers, so it keeps the placeholder.
- Calling the allocation function to stand for the library's own state: nothing says how much it
  asks for or when.
