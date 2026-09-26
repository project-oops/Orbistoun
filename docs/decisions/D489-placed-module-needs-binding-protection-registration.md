# D489 - A placed module must be bound, protected and registered before it runs

**Status:** decided
**Date:** 2026-09-03

A title's own module becomes reachable only once its imports are bound to the module ahead
of the generic stub table, its pages are marked executable after relocation, and its address
range is registered with the fault reporter and the guest's own memory-query calls.

**Why:** any one of the three missing leaves the module unreachable or unreportable: an
unbound import still answers a stub, an unprotected page cannot execute what was just
relocated into it, and an unregistered range is reported as orbistoun's own code rather than
the guest's when the guest faults inside it.

**Rejected:**
- Treating placement, binding, protection and registration as one step: each was
  implemented separately and each omission produced its own misleading failure, which argues
  for keeping all four checked rather than assumed to follow from placement.
