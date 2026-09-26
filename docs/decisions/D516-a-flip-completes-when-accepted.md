# D516 - A flip completes when it is accepted

**Status:** decided
**Date:** 2026-09-03

With no scanout and no vertical blank, a submitted flip completes as soon as it is accepted, so
`sceVideoOutIsFlipPending` answers zero for any open port and refuses a handle no port owns.

**Why:** the headless flip model already drives `sceVideoOutGetFlipStatus`; a queue that empties
on submit has nothing pending at any moment a guest can observe. The count is a number a guest
spins on, so a placeholder there reads as millions of pending flips and holds a present loop
forever.

**Rejected:**
- The stub placeholder: reads as a huge pending count and the guest never leaves its frame loop.
- Answering zero for an unknown handle: tells a guest that a port it does not have is idle.
- Modelling a vertical-blank delay: there is no scanout to take its timing from.
