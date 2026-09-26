# D459 - A call trace records what an answer was, not only what was asked

**Status:** decided
**Date:** 2026-09-01

Recording a guest library call records the value the implementation answered alongside the
arguments the guest passed. A call whose answer is not yet known, or is not known to be safe to
read, reports unknown rather than a default value.

**Why:** An implemented call answering a wrong value the guest goes on to trust is a real and
common class of fault, and until the answer itself is recorded a reader has to reconstruct it by
reading the implementation's code and guessing. Any value can be a legitimate answer, including
zero, so a slot that never got an answer must be distinguishable from one that answered zero.

**Rejected:** recording only a boolean success/failure - loses exactly the value that turns out to
be the wrong one.
