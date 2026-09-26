# D286 - The argument sweep crosses each slot with a success return

**Status:** decided
**Date:** 2026-08-26

`sweep` plants each argument slot both unforced and with the call forced to answer success,
keyed `(slot, answer)` with the unforced run first. An out-parameter finding carries the
answer it was found under.

**Why:** a guest checks the return before reading an out-parameter, so each half alone is a
clean negative and two clean negatives read as proof of absence. Only the success value gates
that read, so the return is a condition, not a sentinel to difference. A finding needing one
intervention is stronger than one needing two, and a finding without its condition cannot be
reproduced.

**Rejected:**
- One axis at a time: cannot see a two-condition dependency.
- Crossing argument sentinels with return sentinels: a product of two differencing questions the guest never asked.
