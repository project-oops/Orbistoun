# D424 - A flip completes the instant it is submitted

**Status:** assumed
**Date:** 2026-08-31

`sceVideoOutSubmitFlip` completes synchronously: the port's completed-flip count advances on
submit, and `sceVideoOutGetFlipStatus` presents that count at the structure's documented leading
field. No other field of the status structure is written.

**Why:** A headless run has no scanout and no vertical blank to advance a queued flip later, so
completing on submit is the only model available; a guest polling for the count to move sees it
move. The rest of the flip-status layout has no citable source, so it is left unwritten rather
than guessed.

**Rejected:** modelling a pending-flip count that only clears on a simulated vertical blank - no
lawful source describes the timing, and nothing measured needs it.
