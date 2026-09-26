# D225 - Probe answers are returned unless they are handles

**Status:** decided
**Date:** 2026-08-24

When orbistoun cannot say what a function does it may ask a probe. The answer is recorded with
the caveat that it came from the probe's state, not the guest's, and returned to the guest unless
the function returns a handle or pointer, which is recorded but not returned.

**Why:** an unimplemented stub is certainly wrong, while a measured value under different state is
probably close and honestly labelled. A handle from the probe's address space is meaningless here
and would be dereferenced much later. The return kind is already recorded and checkable, unlike a
judgement of purity.

**Rejected:**
- Returning every answer: foreign pointers dereferenced.
- Returning only for functions judged pure: an unreliable judgement made in advance.
