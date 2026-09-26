# D310 - The worker reads requests on its own thread

**Status:** decided
**Date:** 2026-08-27

The worker reads requests on a dedicated thread while the guest runs. A shell request is
applied on that thread and produces no reply, so the event stream keeps exactly one writer.

**Why:** guest code runs in a child process, and a request read only between runs arrives
after the run it was meant to interrupt. A reply from the reader thread would interleave with
the run's own events and corrupt the stream only under the timing this exists for, so the
property is asserted as no event.

**Rejected:**
- Send-once-then-listen: no in-flight control except termination.
- Replying to shell requests: a second writer on the stream.
