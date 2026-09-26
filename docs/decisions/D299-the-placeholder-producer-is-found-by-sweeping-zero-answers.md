# D299 - The placeholder producer is found by sweeping zero answers

**Status:** decided
**Date:** 2026-08-26

When a guest faults on one of this project's own placeholder codes used as an address, the
loop forces each unimplemented import in the trace to answer zero in turn and keeps the one
whose run stops faulting on a placeholder. That patch changes only an answer, so it may be
kept on `FURTHER` (D296).

**Why:** a guest treating an answer as an address is itself the evidence that the function
returns something dereferenceable, and zero is what such a caller tests for. The candidates
are already in the trace and a boot costs a fraction of a second, so the sweep is exhaustive
and the oracle needs no judgement.

**Rejected:**
- Asking a person to find the producer: the report cannot name it, and a sweep can.
