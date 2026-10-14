# D885 - A frame switched from waits on the device until it is read

**Status:** measured
**Date:** 2026-10-14

**Amends:** D714 - a submission drawing into another target than the pending one wrote the pending
frame back first, and a submission drawing depth alone wrote back any pending frame.

When a submission draws into a colour target other than the one whose frame is pending, the pending
frame is let go of as a flip lets go of it (D719): a deferred copy onto its own target, kept in a
device snapshot, its pages guarded, carried out by the first touch of them. A submission drawing
depth alone lets go of it the same way. A later draw into that target takes the frame as held
(D801) and supersedes the copy unread; a covering fill drops it; anything that reads or writes its
memory first carries it out, so every byte it produces is the one the immediate write-back gave.
Where it cannot wait, it is written back at once as before.

A target whose frame was held at a switch and then carried out is written back at every later
switch instead: something reads it, so keeping it costs a snapshot and a guard on top of the
write-back. The last sixteen targets held at a switch are remembered, each with whether one of
their frames was carried out.

**Why:** PPSA02664 alternates its draws between targets. Each switch wrote the frame back - 1,035
write-backs of 3.5 ms in 30 s - and the next draw into the target found memory unchanged, so the
frame was never read from memory. `orbistoun-cli ab` against b5bbe117, three rounds of 30 s:
43.1 -> 60.0 fps, `write target` 12.6% -> 0.6%, `read back` 5.5% -> 0.4%. SuperTuxKart's targets
are mip levels sampled through memory: kept at every switch, each was carried out by its first
read, and it ran 54.0 fps against 61.1 (five rounds of 75 s). Written back once read, it runs
73.9 against 70.5. GLTC00001's write-backs are one per submission, not at a switch, and are
unchanged (34.8 against 35.5, within its spread). The test
`a_frame_switched_from_is_kept_until_read_then_written_at_the_switch` checks the frame is kept,
that its first read finds it in the target's byte order, and that the next switch writes it back.

**Rejected:**
- Keeping every switched-from frame: exact, but SuperTuxKart paid for a snapshot and a guard on
  every frame and then carried each out, 12% slower.
- Keeping only a target a texture has bound whole: PPSA02664 never samples its targets, so nothing
  was kept and the gain was lost.
- Reading a texture that is exactly a kept frame off the device, without carrying the copy out:
  exact, but neither PPSA02664, GLTC00001 nor SuperTuxKart binds one (its sampled targets are mip
  levels sharing 64 KiB blocks), and it cost two locks on every texture bind.
