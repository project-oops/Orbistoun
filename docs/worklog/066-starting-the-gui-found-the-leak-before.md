# Starting the GUI found the leak before writing a line of GUI


Phase 2b, `orbistoun-gui`, on egui. The first thing it did was justify itself: within
minutes of looking at what a second shim would need, `cmd_run` in the CLI turned out to own
the whole run orchestration - worker spawn, event collection, before/after comparison,
verdict. None of it reachable from anywhere else, all of it needed by the GUI (D160).

Principle 13 predicts exactly this and says the way you find it is by having more than one
shim. There were two consumers of the service, written by the same hand at the same time,
which tests nothing.

The fix was a layering move rather than a rewrite. `orbistoun-worker` depends on
`orbistoun-service`, so the service cannot depend back - and the trace types all lived in
`orbistoun-worker::report`, above the layer that needed them. The shapes moved down to
`orbistoun-report::trace`; the fault handler, region table and allocation-free line writer
stayed put. `compare(before, after)` now sits beside `RunDiff::between`, which was already
doing the same kind of thing in the same crate.

The shims keep printing and nothing else. `Verdict::label`/`summary` live on the type, so
two shims cannot describe one measurement two ways. Seven tests on the comparison,
including the two-signal case from D129 that was previously protected by nothing.

Verified behaviour-identical afterwards: PPSA28061 still 46 imports, `image+0xecda`, all
799 calls conforming.

**Blocked from here.** `orbistoun-translate` does not compile - a duplicate `Debug` derive
on `predicated::Mask` from the GPU thread's in-flight work - and `orbistoun-cli` depends on
it transitively, so the GUI crate cannot be built or run until that clears.
`orbistoun-report` is independent and green: 34 tests, no lints. Not touched, per the
standing rule about the other thread's crates.

