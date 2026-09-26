# orbistoun-worker

Worker mode: hosting the crates behind the protocol, and driving a worker from a shim.

It holds both halves of the child-process arrangement: `serve` (the child) and
`WorkerHandle` (the parent). A `Run` request loads the module, executes it, catches the fault
if there is one, and returns the phases reached with a terminal `Outcome`. The run report -
trace, progress verdict, fault detail and ranked findings - is assembled here (`report.rs`)
from what the dispatch layer recorded, using [orbistoun-report](../orbistoun-report/)'s
types. It speaks [orbistoun-proto](../orbistoun-proto/).

## Isolation

A guest fault is an access violation in the process that runs it, so the guest runs in the
child and the parent survives to write out what was learned. A run that killed the tool would
lose the trace, which is the only thing the run was for.

## Rules

- **Self-reinvocation.** The parent re-invokes the running executable with a hidden
  `--worker` flag (`WORKER_FLAG`) rather than spawning a separate worker binary (D033). The
  worker is the same build, so version skew is impossible by construction. Worker mode is a
  mode any shim can enter, and it is as thin as the other shims.
- **A failing request does not end the session.** Request errors come back as `Failed` and the
  loop continues; exiting on the first bad request would turn a recoverable problem into a
  lost session.
- **A version mismatch ends the session.** Continuing would parse every later message against
  the wrong contract.
- **A closed control channel ends the process.** When the channel closes without a `Shutdown`,
  the parent is gone and there is nobody to report to, so the worker exits with
  `EXIT_ORPHANED`, even with a guest running, and never outlives the window that launched it.

## Tests

`serve` takes a reader and a writer rather than real stdio, so the whole protocol loop runs
over in-memory pipes with no process spawned. Spawning is covered by integration tests in
`orbistoun-cli`, so a protocol bug and a process bug stay distinguishable.
