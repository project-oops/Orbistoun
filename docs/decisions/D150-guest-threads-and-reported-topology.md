# D150 - Guest threads and reported topology

**Status:** decided
**Date:** 2026-08-20

The guest decides how many threads exist and each is a host thread; the host decides how fast
they run. `CpuTopology` reports the target's shape by default, not the host's. Affinity follows
`AffinityPolicy` - `Observe` by default, `Map` folds guest cores modulo the host's, `Strict`
applies or refuses - and the request is recorded whichever applies.

**Why:** a slower host runs the same program more slowly, which is correct rather than a
failure. A title asking about cores is asking about the machine it was designed for. No title
has been shown to depend on placement, so a mapping invented first would read as a measurement.

**Rejected:**
- Enforcing a minimum core count: refuses a correct, slower run.
- Reporting the host topology: sizes thread pools for an untested machine.
- Clamping out-of-range cores: merges threads the guest separated.
