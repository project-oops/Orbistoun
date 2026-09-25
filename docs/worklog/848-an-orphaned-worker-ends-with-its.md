# 848. An orphaned worker ends with its parent

**2026-09-24**. A GUI worker (`orbistoun-gui.exe --worker`) was found still running a guest about
an hour after its window had closed. It held `target/release/orbistoun-gui.exe` open, so the
release build failed with "Access is denied".

- **Cause:** the worker's stdin reader thread saw EOF and just returned. The main thread was inside
  the guest, so it never saw the channel close. Nothing else ties a worker to its parent.
- **Fix (D715):** `serve` takes an `on_hangup` callback, which the reader thread calls when stdin
  ends without a `Shutdown`. The worker process passes `end_orphaned_worker`, which writes a line
  on stderr and exits with `EXIT_ORPHANED` (3). A killed parent's pipe also ends as EOF on Windows,
  so no job object and no new `unsafe` were needed.
- **Tests:** `an_input_that_ends_without_a_shutdown_is_a_hangup` and
  `a_shutdown_is_not_a_hangup`.
- **Verified live:** started the release GUI with `--title GLCB00001`. The worker drew frames and
  used 30 s of CPU. `taskkill /F` on the GUI left no `orbistoun-gui.exe` within 5 s, and the
  worker's stderr ended with the new line.
