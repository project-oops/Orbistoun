# D814 - the flip status reports the last flip argument

**Status:** decided
**Date:** 2026-10-12

`sceVideoOutGetFlipStatus(handle, status)` writes the port's completed-flip count at +0, as
before, and the argument of the last completed flip at +0x18, as a 64-bit word. Nothing else in
the structure is written.

`known_by = assumed` for +0x18. The open toolchain's declaration of `SceVideoOutFlipStatus` puts
`flipArg` there, the fourth 64-bit word, after the count, the process time and the timestamp.
Every status obSCEne has read (`080-video/visual-flip`, `report-1791801495.txt` 4424-4433) held 0
there after flips whose argument was 0, which agrees with it and does not distinguish it.
REQ-fs01 asks for the status after flips with distinct arguments.

**Why:** PPSA25872 paces its frames by that word. Its flip thread reads the status on every flip
event, takes +0x18 as the latest completed flip's argument with bit 63 cleared, and releases as
many frame slots as it moved since the last one it saw. With only the count written, the word was
whatever the title's stack held, no slot was ever released, and the main thread waited for one
after its second frame for the rest of the run.

**Rejected:**
- Writing the rest of the structure: the times, queue counts and current buffer read in the
  measured rows are a console's at that moment, and no title here has been seen to read them.
