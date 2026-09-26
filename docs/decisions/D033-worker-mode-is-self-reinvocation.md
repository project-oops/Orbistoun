# D033 - Worker mode is self-reinvocation

**Status:** decided
**Date:** 2026-09-26

The worker is the same executable re-invoked with a hidden `--worker` flag, not a separate
binary and not a clap subcommand. Every shim runs guests through it; `--in-process` exists only
as a debugging aid. A failing request is reported and the loop continues; a protocol version
mismatch ends the session.

**Why:** the same executable cannot skew versions, and no shim is privileged. One execution
path means the GUI's protocol is exercised on every CLI run. A failed request is recoverable,
while a version mismatch would misparse every later message.

**Rejected:**
- A separate worker binary: version skew and a privileged shim.
- A CLI fast path in-process: a second execution path that goes untested.
- A user-facing subcommand: invites driving the worker by hand.
