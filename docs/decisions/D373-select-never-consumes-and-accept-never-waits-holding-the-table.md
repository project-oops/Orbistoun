# D373 - select never consumes, and accept never waits holding the table

**Status:** decided
**Date:** 2026-08-29

`select` keeps the connection it accepted to test readiness on the listener, and the guest's
`accept` takes it first; a stream is tested with `peek`, and zero bytes counts as ready.
`accept` clones the listener and waits after releasing the descriptor table. Readiness is polled
a millisecond apart, and the knowledge file says so.

**Why:** the host library has no readiness primitive, so asking means accepting, and a naive
`select` takes the connection it reports. End of file is ready, or a guest parks on a closed
connection. Waiting while holding the table blocks every file call, including the `select` that
would report the connection. A latency a real `select` lacks must be stated.

**Rejected:**
- Discarding the probed connection: the guest waits for one that was taken.
- Blocking inside the table lock: deadlocks a second guest thread.
