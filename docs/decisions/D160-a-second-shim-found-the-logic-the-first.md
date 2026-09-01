# D160 - A second shim found the logic the first one had absorbed


**decided** · 2026-08-20

Principle 13 says shims hold no logic and that if one starts to, the others are already
drifting. It also says the way you find out is by having more than one. There were two
consumers of `orbistoun-service` - the CLI and the worker - and both were written by the
same hand at the same time, which proves nothing at all.

Starting a third, `orbistoun-gui`, found the leak within minutes: **`cmd_run` in the CLI
owned the entire run orchestration.** Spawning the worker, reading the previous trace,
driving the request, collecting events, comparing before against after, and computing the
verdict - all of it in a shim, none of it reachable from anywhere else. A GUI would have
had to reimplement the comparison, and two shims computing "did this change help?"
separately is precisely how they come to disagree about the only number this project
steers by (D080).

### Why it could not simply be moved to the service

`orbistoun-worker` depends on `orbistoun-service`, so the service cannot depend back. And
the trace types - `CallTrace`, `FaultSite`, `AbiReport`, `TracedCall`, `Registers`,
`CalledImport` - all lived in `orbistoun-worker::report`, next to the fault handler that
fills them in. Sensible-looking, and it put them above the layer that needed them.

So the *shapes* moved down to `orbistoun-report::trace`, which the service layer and both
shims can all see. The *producing* side stayed in the worker: the fault handler, the region
table, the allocation-free line writer. Only data and pure comparison moved.

`compare(before, after) -> Progress` now lives beside `RunDiff::between`, which was already
doing the same kind of thing one layer down - so the new home was not invented for this,
it was already there and unused for it.

### What the shims keep

Printing, and nothing else. `Verdict::label` and `Verdict::summary` are on the type rather
than in a `println!`, so the CLI and the GUI cannot describe one measurement in two
different ways.

The comparison is pure and now has seven tests, including the case that forced two signals
in the first place: eight more subsystems reached behind an instruction pointer that had
gone backwards (D129). That case is a one-line mistake to reintroduce and was previously
protected by nothing.

### The general point

The leak was invisible for as long as there was one shim, and it was not caused by
carelessness - `cmd_run` is a perfectly reasonable function to write. **The architecture
did not fail; the test of it was missing.** A seam with one consumer is a claim.

