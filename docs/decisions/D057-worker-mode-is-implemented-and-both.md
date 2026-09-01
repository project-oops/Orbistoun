# D057 - Worker mode is implemented, and both shims go through it

**decided** · 2026-08-19

D033's design, now real. `orbistoun-worker` holds both halves: `serve` is the child,
`WorkerHandle` is the parent, and the parent re-invokes **the running executable** with
a hidden `--worker` flag. Verified end to end - the binary spawns itself, handshakes
across a process boundary, survives request failures, and several workers coexist.

`--worker` is deliberately **not a clap subcommand**. It is an implementation detail of
how shims execute guests, not a user-facing verb, and listing it in `--help` would
invite driving it by hand.

**Two failure policies, and they differ on purpose:**

- A **failing request** is reported as `Event::Failed` and the loop continues. A worker
  that exited on the first bad request would turn a recoverable problem into a lost
  session.
- A **version mismatch** ends the session. Continuing would parse every later message
  against the wrong contract - far harder to diagnose than an outright refusal.

**Testability was designed in, not retrofitted.** `serve` takes a reader and a writer
rather than reaching for real stdio, so the protocol loop is exercised over in-memory
pipes with no process at all; spawning is covered by separate integration tests in the
CLI. A protocol bug and a process-spawning bug therefore stay distinguishable, instead
of both surfacing as "the worker did not respond".

`stderr` is inherited rather than piped: the worker's log should reach wherever the
shim's does, not vanish into a pipe nobody drains.

