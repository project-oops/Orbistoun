# D147 - Unaddressable descriptors are forced out of bounds

**Status:** decided
**Date:** 2026-09-26

Buffer accesses are emitted as run-time arithmetic over the descriptor. A descriptor asking for
addressing this translator does not implement - swizzled records, or the lane number folded into
the index - is forced out of bounds, so reads give zero and writes are dropped.

**Why:** a translated shader cannot refuse at run time. The unswizzled address would read
real-looking data from the wrong offset, indistinguishable from correct output; a buffer that
reads entirely zero is a symptom anyone can see.

**Rejected:**
- Ignoring the unsupported mode: plausible data from the wrong place.
