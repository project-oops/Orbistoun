# orbistoun-worker

Worker mode: hosting the crates behind the protocol, and driving one from a shim.

**Models:** both halves of the child-process arrangement - `serve` (the child) and
`WorkerHandle` (the parent).

**Deliberately fakes:** nothing. `Run` loads the module, executes it, catches the fault
if there is one, and returns the phases reached with a terminal `Outcome`.

**Design note.** The parent **re-invokes the running executable** with a hidden
`--worker` flag rather than spawning a separate worker binary (D033). The worker is
then literally the same build, so version skew is impossible by construction. No
binary is privileged: worker mode is a mode any shim can enter, and it is as thin as
the other shims.

Two failure policies worth knowing:

- **A failing request does not end the session.** Request errors come back as
  `Failed` and the loop continues - a worker that exited on the first bad request would
  turn a recoverable problem into a lost session.
- **A version mismatch does end it.** Continuing would parse every later message
  against the wrong contract, which is far harder to diagnose than a refusal.

**Testability.** `serve` takes a reader and writer rather than reaching for real stdio,
so the whole protocol loop runs over in-memory pipes with no process spawned. Spawning
is covered separately by integration tests in `orbistoun-cli`, so a protocol bug and a
process bug stay distinguishable.

**Status:** done. The protocol loop, process management, and execution all work, and the
run report - the trace, the progress verdict, the fault detail, and the ranked findings -
is assembled here (`report.rs`) from what the dispatch layer recorded.

**Isolation is the point.** A guest fault is an access violation in this process, so it
happens in the child and the parent survives to write out what was learned. A run that
killed the tool would lose the trace, which is the only thing the run was for.
