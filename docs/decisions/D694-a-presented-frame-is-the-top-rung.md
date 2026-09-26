# D694 - A presented frame is the top rung

**Status:** decided
**Date:** 2026-09-26

`Reach::Presented` sits above `Flipped` and is awarded when a run flipped a buffer holding
pixels the guest wrote.

**Why:** a flip is a place reached, not a picture shown, and a ladder whose top rung everything
reaches says nothing about the work left. A buffer differing from its prior contents is a
positive measurement, and it is the framebuffer comparison the project already trusts. `Flipped`
stays a real distance: an output opened, configured and flipped against real implementations.

**Rejected:**
- Replacing `Flipped`: loses the distance between flipping and faulting.
- Treating a flip as presentation: advertises output that does not exist.
