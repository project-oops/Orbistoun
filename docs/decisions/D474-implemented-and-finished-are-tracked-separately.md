# D474 - Implemented and finished are recorded as separate states

**Status:** decided
**Date:** 2026-09-02

An imported function's knowledge entry may declare what it does not yet handle, and the gap
report lists that declaration separately from whether the function resolves to code at all.

**Why:** several functions answer only the easy case and already document that in a comment,
but a caveat nobody has to read past does not affect whether a report counts the function as
done. A declared-gap field lets that knowledge reach the report instead of staying in a
comment the report never consults.

**Rejected:**
- Counting a function as complete once it resolves to any code: conflates "handles nothing
  yet" with "handles the general case," and cannot tell the two apart without reading every
  implementation by hand.
