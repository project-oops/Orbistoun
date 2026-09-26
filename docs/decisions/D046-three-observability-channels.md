# D046 - Three observability channels

**Status:** decided
**Date:** 2026-08-19

The developer log (`tracing`) says what the emulator is doing; the guest call recording holds
every guest call and never goes through `tracing`; the run report is a versioned, bounded,
machine-readable record per run. The run report is the contract; log prose is not.

**Why:** a log call per guest call would dominate the profile and change what it measures. A
consumer grepping log prose breaks when a message is reworded. The report is an index - top-N,
last-N, its own inputs embedded, a diff against the previous run - because a finite reader
cannot consume a full trace.

**Rejected:**
- One channel for everything: volume and audience differ by orders of magnitude.
- Agents parsing logs: an unversioned API.
