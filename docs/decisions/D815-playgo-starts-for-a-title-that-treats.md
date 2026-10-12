# D815 - PlayGo starts for a title that treats it as optional

**Status:** decided
**Date:** 2026-10-12

`scePlayGoInitialize(param)` answers 0 for a parameter block, writing nothing, and
`scePlayGoTerminate()` answers 0 once after it. A null block, and a terminate with nothing
initialised, answer the placeholder. Nothing else in `libScePlayGo` is implemented, so
`scePlayGoOpen` answers the placeholder.

Guest-observed (`known_by = guest-observed`). The library binds in no probe leg, by link or by name,
on a non-retail eboot (REQ-pg01), so its answers cannot be measured. PPSA21564 initialises it once at
start-up with a 0x200000-byte work block, asserts the answer is 0 (`PlayGo.cpp` line 111) and traps
with `int 0x41` when it is not. The retail title runs on the console (D709), so the console answers
0 there.

What follows needs nothing invented. When `scePlayGoOpen` fails, the title sets its PlayGo state to
3 and goes on, the same state it reaches when `scePlayGoGetChunkId` lists one chunk: the content is
taken as installed. Every file of the title is present to orbistoun, so that is what it is. On that
path it terminates PlayGo and asserts that answer is 0 too (line 127).

**Why:** the placeholder is not 0, so PPSA21564 stopped at its own assertion at start-up.

**Rejected:**
- Answering `scePlayGoOpen`, `scePlayGoGetChunkId` and `scePlayGoGetToDoList`: the handle, the chunk
  list and the to-do list are the library's from the package's chunk table, and the title already
  goes on without them.
