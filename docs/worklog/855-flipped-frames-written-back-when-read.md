# 855. Flipped frames written back when read, and shown from the device

**2026-09-25**. D719. Neverball goes from 12 to 14 flips a second. Its presented frames and
gl1-probe's results are unchanged.

**What changed:**

- **The flip defers its write-back** as a deferred copy of the frame onto its own target (D717's
  machinery): snapshot, guard, carried out on first touch.
- **A command-processor fill covering a deferred destination drops it unread.** It releases the
  guard and discards the snapshot. The GL context clears each target at the start of its frame, so
  in Neverball no flipped buffer is ever written back: `carry out` is 0/s.
- **The window takes the frame from a second device snapshot**, on an `orbistoun-present` thread.
  Each pixel goes through `memory_order` (the target's swap), then `scanout_rgba`. The guest
  thread's present went from ~70 to ~0 ms/s, and the flip from ~86 to ~25 ms/s.
- **`page_guard` spans host regions.** Neverball's 8 MB targets lie across five 2 MB mapped views,
  so the one-region guard refused every flip, and every flip silently fell back to the immediate
  write-back. Found by instrumenting the declining branches.

**Checked:**

- A temporary comparison of every device-shown frame with the frame read and detiled from memory:
  0 differing pixels in 116 frames. The same comparison with the target swap dropped differed in
  ~98% of pixels, so it could fail.
- `a_flipped_frame_is_written_back_on_first_read_or_dropped_by_a_covering_fill` was watched failing
  with the deferral disabled and with the drop disabled.
- `a_range_across_two_allocations_is_guarded_and_released_whole` covers the guard.
- gl1-probe verdicts and pixels are identical. **It is not a check of this**: it flips once a run,
  and its front-buffer tests fail before and after.

`carry out` is a new `ORBISTOUN_PERF_DETAIL` span.
