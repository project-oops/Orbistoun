# 857. A uniform target skips its detile, and the window is compared, not hashed

**2026-09-25**. Neverball runs at 14-16 flips a second. Its frames and gl1-probe's results are
unchanged.

**A target of one word throughout is handed to the drawer as that word everywhere**
(`read_target`). A clear leaves the target uniform, and every frame's first read of its freshly
cleared target was an 8 MB detile. Every pixel of a uniform surface is the word in the target's
byte order, wherever the tiling puts it. Read target went from ~55 to ~24 ms/s.
`a_uniform_target_reads_as_its_detile_would` compares the shortcut with a real detile of the same
`SWAP_ALT` memory; it was watched failing with the swap dropped.

**The guest window's change is found by comparing, not hashing** (`set_guest_memory`). The words are
compared, a generation counts changes, and the upload happens when the generation moved. The copy
reuses its allocation. This is exact where equal hashes only probably meant equal words, and cheaper:
`guest window` went from ~32 to ~8 ms/s. The existing re-upload test covers it.

**A seed goes into the resident attachment in place** (`ResidentAttachment::reload`). The first
submission into a changed target seeds the device with it. That used to destroy the attachment
(waiting for the device to go idle) and create a new one (two 8 MB allocations and a waited upload),
once a frame. Now it settles, writes the attachment's own buffer and records the copy unwaited in
queue order. It is still a copy, not a device clear: a float clear's rounding to unorm is left
partly to the implementation. The seed's words also become bytes in one copy, not a byte at a time
through an iterator. The device went from ~232 to ~210 ms/s, and Neverball runs at 14-16 flips a
second.

**Measuring:** new `ORBISTOUN_PERF_DETAIL` spans `whole: window`, `whole: pipeline` and
`whole: record` split a whole-path draw.

**Tried and removed: recomputing a draw's state only from the registers written since the draw
before.**

- Counted on Neverball, the GL context writes ~13 registers between draws, and the fragment
  stage's user data before every one of them.
- Skipping unchanged groups skipped nothing, and keeping the list made prepare's geometry span
  slower (~175 to ~197 ns a draw).
- A per-draw saving has to come from making each lookup cheaper, not from skipping lookups.

**Where the rest goes:**

| where | ms/s |
|---|---|
| a whole-path draw's window upload (a real copy into device-visible memory) | ~19 |
| whole-path recording | ~15 |
| whole-path draws in total (~1,300/s, one per state change) | ~60 |

- The device thread is busy ~35%, mostly waiting on the GPU (~168 ms/s) and driver submits. The
  GPU work per submission is loading and storing the attachment for its pass, plus the readback
  copy's snapshot.
- The executor's per-submission stderr line costs ~21 ms/s. It belongs to the logging-levels work.
