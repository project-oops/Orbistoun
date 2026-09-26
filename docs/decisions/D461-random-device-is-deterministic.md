# D461 - std::random_device is deterministic

**Status:** assumed
**Date:** 2026-09-01

The entropy primitive behind `std::random_device` answers a fixed-seed sequence, advanced
once per call, rather than a physically random value.

**Why:** every measurement this project takes depends on two runs of one build behaving
identically. A guest reads this call only to seed its own generator, so a well-distributed
value serves it as well as an unpredictable one, and only the deterministic choice keeps a
run reproducible.

**Rejected:**
- A non-deterministic source, as the standard intends: correct in isolation, but it puts
  non-determinism back into exactly the layer this project removes it from.
