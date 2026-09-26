# D685 - A deliberate exit earns a rung below a flip

**Status:** decided
**Date:** 2026-09-14

`Reach` has `Exited` between `Entered` and `Flipped`, reached by a guest that leaves through
`exit`. A guest that both flipped and exited is recorded at `Flipped` with the exit kept in its
outcome; abort, an unhandled signal and the time limit stay at `Entered`.

**Why:** exiting is a call the guest made with a status it chose, not mere survival, so it
passes the test a rung must pass. It is also trivially reachable, and a rung dominates every
quantity below it: above `Flipped`, a program whose first instruction is `exit` would outrank
titles that render.

**Rejected:**
- No rung for exiting: a deliberate stop reads the same as a fault.
- Above `Flipped`: the conformance harness would head the frontier.
- A rung for surviving to the time limit: not dying is an outcome, not a distance.
