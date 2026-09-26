# D303 - The title corpus and the probe are the acceptance oracle

**Status:** decided
**Date:** 2026-09-26

A candidate change is scored against every title on the machine and the conformance probe
together. It is kept only if no probe check goes from pass to fail, something improves, and no
guest starts derailing into non-code; where a probe check covers a function, its verdict
outranks reach.

**Why:** the generator can be as simple as an enumeration only if the oracle cannot be fooled,
as with the hash in the naming loop. The probe grades against a specification but covers only
what someone wrote a check for; every title is an independent guest with its own expectations,
and several agreeing is a relationship where one is a coincidence. A derailed guest was broken
by the change, not helped.

**Rejected:**
- Reach on one title as the gate: saturates and coincides.
- The probe alone: never covers every entry point.
- A cleverer generator behind a weak oracle: produces confident wrong answers.
