# 696. The presented rung gets its arm, awarded by a framebuffer readback

**2026-09-19** — inbox `-9b1f`: `Reach::Presented` was declared (worklog on the rung itself) but
nothing awarded it, because `status_of` had no arm and there was nothing to read back. `-420c` last
tick made the flipped buffer readable from outside the video crate; this wires the rung to it. Six
guests sit at `flipped` with full standing and none has shown a pixel - the scale can now say
`presented` when one does, and keeps saying `flipped` until one does.

## The rung, driven by a measurement

- **The field.** `CallTrace` gains `frame_written: bool` (`#[serde(default)]`, beside `frames`).
  `frames` counts flips a port accepted; `frame_written` says one of those flips carried a buffer the
  guest actually wrote into.
- **The arm.** `status_of` awards `Reach::Presented` for `reached == "Entered" && frames > 0 &&
  frame_written`, placed above the plain `frames > 0 => Flipped` arm as the stronger, more specific
  claim.
- **The worker sets it, bounds-checked.** `flipped_frame_written` (orbistoun-worker) takes the
  last-flipped buffer's guest address from `orbistoun_video::last_flipped_buffer()` (`-6a86`/`-420c`),
  bounds-checks it against the run's own allocated regions, and reads a bounded head window back: an
  unwritten buffer is the zero a fresh allocation holds, a written one differs at its start. A guest
  can flip a buffer index registered with any address, so the range is checked before a byte is read -
  the D101 boundary `-5bff` put on the submit path, here on the report path. The buffer's true extent
  needs the attribute struct decoded (unmeasured), so the read is bounded to one page.

## Made to fail, both directions

- **The rung (report crate).** The old test `the_top_rung_is_out_of_reach_until_a_buffer_can_be_read_back`
  asserted nothing could reach the top rung - true only while nothing did. Replaced by
  `a_written_frame_reaches_presented_and_an_unwritten_flip_stops_at_flipped`: a written frame reaches
  `Presented`; the same run with nothing written stops at `Flipped` (why the corpus is unchanged); a
  written buffer nobody flipped stays at `Entered`; and nothing below `Entered` presents even with a
  frame written and flipped.
- **The readback (worker crate).** Split into a pure `flipped_window` (bounds arithmetic, tested for
  in-region / clamped-to-end / past-end / unallocated / no-region) and `frame_written_against` (bounds
  + the read), the latter exercised on a **real** host buffer: all-zero reads as unwritten, one written
  byte flips it to written, and an out-of-region address is refused without a read. This is the guard
  that proves `frame_written` can be set at all rather than being a field that is always false
  (principle 3) - the pure decision plus a thin effectful wrapper (principle 8).

The corpus is unchanged by the change alone: `frame_written` defaults false, no title was re-run, and
no `compat/*.toml` moved. Every title stays at `flipped` until a run with a written, readable frame
says otherwise - which needs the renderer 36c0 tracks.

## A latent doc gate, cleared on the way (see worklog 697)

Running `./bin/orbistoun check` to completion for the first time in several ticks surfaced a doc step
that had been failing on **pre-existing, committed** intra-doc-link errors in orbistoun-gpu and
orbistoun-gpu-vulkan - unrelated to this change, latent because recent ticks failed an earlier gate and
never reached the doc step. Cleared in worklog 697 so this change could be verified against a fully
green gate.

## Gate state

`orbistoun-report`/`orbistoun-worker` clippy `-D warnings` clean; workspace tests pass (two new tests,
one replaced); `orbistoun prose` exit 0; `status --check` exit 0 (no count moved - no new
implementation, knowledge or declaration); `knowledge-audit` 27 pass; identity scan clean; and with
worklog 697's fixes, **`./bin/orbistoun check` passes end-to-end** (all checks passed). No commit.
