# 850. Lazy readback copies

**2026-09-24**. D717, built and measured.

- **Why:** the refreshed Neverball package submits about 40 times a frame, and each GL submission
  copies the 8 MB colour target into its readback buffer. Writing the frame back before every copy
  (worklog 849) was exact and ran at 2-3 fps.
- **How:** a copy of the whole pending target keeps a device-side snapshot (`VulkanBackend::
  snapshot_last_frame`, pooled buffers, unwaited copy) and guards the destination's pages
  (`worker::page_guard`). The `report` fault handler calls `agc_driver::carry_out_at`, which writes
  the exact bytes and retries the access. A copy to the same destination supersedes the earlier one
  unread, and command-processor work carries out any overlapping copy first.
- **One bug found on the way:** `GuestCp::copy` carried out destination overlaps before trying to
  defer, so every copy was produced rather than superseded. The destination check now comes after
  `defer_copy`.
- **Accuracy:** gl1-probe, lazy against eager: 0 verdict differences on the 41 shared checks, and
  all 73 eager pixels identical. It now completes 110 checks in 60 s, against 41.
- **Speed:** Neverball goes from 2-3 to 5-6 fps at ~260 submissions a second. The rest of a
  submission's ~3 ms: keeping a snapshot 0.7 ms (thread spawns included), prepare 0.65 ms, the
  target's unchanged check 0.3 ms.
- Also: `executed_frame` is asked of the backend at the run's end rather than cloned on every
  read-back, the presented frame's colour swap happens during detiling, and frame bytes reach words
  by cast.

**Next:** gl1-probe fails 90 of 110. Many rows read `0xbf800000`. That is the accuracy worklist for
the GL path.
