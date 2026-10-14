# D839 - Save-data memory is a store reads and writes copy through

**Status:** decided
**Date:** 2026-10-14
**known_by:** assumed

`sceSaveDataSetupSaveDataMemory2` creates the title's save-data memory: as many bytes as its
parameter block asks for at +8, zero-filled. For the signed-in user, `sceSaveDataGetSaveDataMemory2`
copies the requested span of it into the request's buffer and `sceSaveDataSetSaveDataMemory2`
copies the buffer into it, both answering 0. A request block holds the user at +0 and a data
block's address at +8; the data block holds the buffer, its length and the offset into the memory,
as obSCEne's `130-layout/savedata-memory-5d20` built them. Before set-up a request answers
`0x809f0001` and for another user `0x809f0011`, as measured; a span the memory does not hold is
refused by name. Nothing persists the memory past the run.

**Why:** the only measurement after set-up came from a probe whose set-up had failed underneath:
the system log beside the rows shows its save mount answering `ExecResult 0x809f8022` for api_id 1
under the homebrew title id PPSA90000 (REQ-sd01, `20261009-215800-eboot.log` 6094-6107). Its
`0x809f0000` for every later read, write and set-up describes memory with no backing save. A
homebrew probe cannot set up a working one (REQ-rf03: `Initialize3` is unusable there). PPSA21564
asserts (`SaveMemory.cpp:118`) when a read answers `0x809f0000` and runs on hardware (D708), so a
working read answers success. With this it passes that assertion and reaches its first submission.

**Assumed:** that a read after a working set-up answers 0 with what was written, and zeros where
nothing was. The second set-up's `0x809f0000` is kept as measured, though it came from the same
failed mount.

**Rejected:**
- Keeping `0x809f0000` for every read: it was measured, but of a failure, and a title that works
  on hardware stops on it.
- Serving `sceSaveDataSyncSaveDataMemory` as success: with no persistence there is nothing to sync,
  so it stays unserved until the memory is kept between runs.
