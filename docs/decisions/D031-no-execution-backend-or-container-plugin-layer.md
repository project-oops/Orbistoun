# D031 - No execution backend or container plugin layer

**Status:** decided
**Date:** 2026-08-19

Guest x86-64 code runs natively, with no execution-backend abstraction, and there is one
container parser, with no format plugin layer.

**Why:** native execution is the architecture, and no second implementation will exist to swap
in. A second container format would be a new parser, not an implementation of a trait. Neither
seam buys testability or swappability.

**Rejected:**
- An execution-backend trait: one implementation, forever.
- A container-format plugin layer: pays no rent.
