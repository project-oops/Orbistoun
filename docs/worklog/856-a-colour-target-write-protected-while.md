# 856. A colour target write-protected while trusted unchanged

**2026-09-25**. D720. Neverball's target read goes from ~82 to ~55 ms/s at 14 flips a second. Its
frames and gl1-probe's results are unchanged.

**Why it was 82:** GetWriteWatch answers only for privately allocated memory. The guest's direct
memory is mapped views, so `watch::mark` returned `None` for every colour target. Every submission's
unchanged check then compared the target's 8 MB.

**What changed:**

- **One target at a time is read-only while trusted** (`protect_target`). It is armed before the
  check or the read, so a write that races either is seen next time.
- **Writes release the protection.** A guest write faults and the handler calls `written_at` (write
  faults only), before `carry_out_at`, so a stale resolved range cannot answer for pages protected
  again since. The command processor's own writes (`fill`, `edit_words`, `copy`, `write_guest`)
  release it first, and so do both deferrals, whose guard would otherwise save "read-only" as the
  protection to restore.
- **Lock order** is the deferred list, then the protection, everywhere both are held.
  `protect_target` holds the list throughout, so no deferral lands between its check and its
  protect.
- `page_guard::protect_writes` (`PAGE_READONLY`) shares `set_all` with `guard`.
- **A fill over the whole pending target drops the frame unwritten** and forgets the kept bytes. The
  fill may leave exactly the bytes the draws started from.

**Tests** (each watched failing):

- `a_protected_target_is_trusted_until_a_write_to_it_faults`;
- `a_fill_over_the_whole_pending_target_drops_the_frame_unwritten`.

The test lazy hooks are shared (`tests::fake`), since the first install wins.
`a_drawn_frame_is_written_back_tiled_over_the_target_s_own_contents` now models its guest write's
fault and takes the serial lock: it had been passing only because no test installed the hooks.

**Left in read target:** the first read of each freshly cleared target, 8 MB read and detiled once a
frame.
