# D948 - Extra colour attachments stay on the device across draw segments

**Status:** decided
**Date:** 2026-10-24
**known_by:** measured

A colour attachment after the first (D825) is written back as the first target's frame is when it
is let go of (D719): after a segment's draws it stays on the device, and its write-back is a
deferred write over its own pages, which are guarded. The first touch of those pages - by the guest,
by the command processor's memory work, by a preparation reading a texture or table there, or by a
colour target read over them - reads the attachment off the device and writes it back before the
touch goes on. Until then, the next segment that draws into the same attachment draws over what the
device holds: nothing is read from memory, detiled, handed over, read back or tiled between them. A
compressed attachment is read afresh, since expanding its keys writes them. Where the device no
longer holds an attachment kept there, it is written back and every attachment is handed over again
from memory. Where its pages cannot be guarded, it is written back at once, as before.

On top of it, where a refused segment is passed over (D896), the command processor does not wait for
the device to record a segment's draws: whether the device draws them or refuses them changes
nothing it does next. The draws go to the device thread, and every later device call - a frame or
attachment read back, a snapshot, a dispatch, a sample count - runs after them there, so nothing
reads what they draw before they have drawn it. A segment the device then refuses is counted as
passed over. Without D896 the command processor still waits, because a refused segment stops the
stream before the packets after it (D705).

**Why:** in Grand Theft Auto V's world every segment of its multi-target passes read each further
attachment from memory before its draws and wrote it back after them: about 1 s of a 7.5 s frame,
108 segments a frame. Waiting on the device to record each segment was another 0.8 s, during which
the command processor could have prepared the next one. Writing an attachment back only when
something touches it is the rule the first target already follows, and its guard catches a guest
read as well as a write.
