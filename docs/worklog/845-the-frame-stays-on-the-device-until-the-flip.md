# 845. The frame stays on the device until the flip

**2026-09-24**. D714: a drawn target is written back at the flip, not after every submission.

- **Executor split:** `DrawExecutor` now answers whether it drew. A `FrameReader` fetches the frame
  when it is written back, at a flip (`write_back_at_this_flip`) or before a submission draws into
  another target. Each submission's draws are sent to the device without waiting.
  `ORBISTOUN_TARGET_WRITEBACK=submit` restores the old per-submission write-back.
- **Profiling-led fixes:**
  - **Target unchanged check:** now in place, with no 8 MB copy or conversion.
  - **Window buffer:** rewritten in place. Recreating it every submission made each cached
    pipeline re-point its binding at its first draw, and wait for the device to do it.
  - **Register sweep:** hands each lookup only the registers it reads.
  - **`DMA_DATA`:** fills and copies are done in place (`CpMemory::fill`/`copy`). The fill had
    built its buffer a byte at a time.
  - **Texture binds:** `BindTexture` carries shared texels and a hash taken once at read. The
    pipeline reads each texture once per submission.
  - **Smaller fixes:** mesh availability and each shader's mesh-ness are computed once, and the
    pipeline key is hashed rather than formatted.
- **Checked:** cube frames 1, 5, 41 and 60 match the baseline except for 14-17 pixels of the HUD's
  timing text.

**Result:**

| | before | after |
|---|---|---|
| GL1 Cube | 11 fps | 25 fps |
| Neverball submissions per second | ~40 | ~215 |
| Neverball draws per second | ~15,000 | ~80,000 |
| Neverball frame rate | ~1 fps | 4-5 fps |

The largest remaining costs per Neverball second are prepare (~230 ms) and the window buffer's
per-submission wait for the device.
