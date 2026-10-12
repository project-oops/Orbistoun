# D802 - a flip completion's event id is the one the guest acts on

**Status:** decided
**Date:** 2026-10-12

`sceVideoOutGetEventId(event)` answers 0 for a flip completion: the event orbistoun posts at a
flip, with filter -13 (`EVFILT_VIDEO_OUT`) and ident `0x0006_0100_0000_0000`, both read on hardware
(obSCEne -4e20, `080-video/visual-flip`). Any other event answers the placeholder.

Guest-observed (`known_by = guest-observed`). The call binds in no probe leg (REQ-ve01), so its
answer cannot be measured. PPSA25872's flip handler waits on its flip queue, takes each event's id,
and goes on to the flip only when the id is 0. Otherwise it waits again. The only events its queue
is registered for are flips (it imports `sceVideoOutAddFlipEvent` and no vblank event), so 0 is the
id the title expects a flip to carry.

**Why:** the placeholder never equals 0, so PPSA25872 waited forever on flips orbistoun had
already delivered. It carried out 2 submissions in 120 s.

**Rejected:**
- Answering 0 for every event: an id for a vblank or any other video-out event is unestablished,
  and a title that tells them apart would take one for a flip.
- Deriving the id from the ident's bits: nothing measured says how the two relate.
