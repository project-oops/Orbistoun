# D697 - The direct-memory pool defaults to the retail size

**Status:** assumed
**Date:** 2026-09-15

`Settings::pool_bytes` sizes both the direct-memory pool and the size query, defaulting to 12
GiB, the size measured for a retail title; a homebrew leg sets the measured 5 GiB homebrew size
in its configuration.

**Why:** both figures are hardware measurements: the platform grants retail titles and homebrew
payloads different budgets. The corpus is retail, and its allocation walls need the retail pool.
A guest sizes its heaps against the query, so the query and the pool must agree, and one field
makes that structural.

**Rejected:**
- One fixed constant: wrong for one class of guest.
- Detecting the guest class automatically: nothing models that signal, and no guest needs it
  yet.
