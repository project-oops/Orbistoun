# D485 - A measured value earns a constant claim only across repeated agreement

**Status:** decided
**Date:** 2026-09-03

A measured constant is asserted as an exact value only once independent captures agree on
it; a value that varies between captures is asserted only as membership in the set of values
actually observed, never as equality with one of them.

**Why:** a single capture, or two that happen to agree, cannot distinguish a true constant
from a value calibrated per boot. Asserting a varying value as an exact constant both invents
a claim about the platform and hides the platform's own boot-to-boot variation.

**Rejected:**
- Asserting every measured value as an exact constant regardless of how many captures
  produced it: a per-boot value then fails every time it is recaptured, or is never
  recaptured and the false claim stands.
