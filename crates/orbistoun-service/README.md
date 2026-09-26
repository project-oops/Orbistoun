# orbistoun-service

The shared logic layer every shim calls.

It assembles the module registry, inspects a container, surveys imports, places and relocates
an image, builds thunks, applies page protection, resolves import labels, discovers titles,
and emits the default stub policy - every operation a shim needs. It refuses when a container
cannot be parsed rather than returning an empty result.

## Rules

- **Shims hold no logic.** `orbistoun-cli`, the GUI and worker mode are interaction shims; this
  is what they all call, so an operation exists once and the shims cannot drift.
- **Everything crossing the boundary is serialisable and owned**, taken from
  [orbistoun-proto](../orbistoun-proto/) rather than defined here. The same operation is then
  invoked in-process by the CLI and across a process boundary by the worker.
- **One module list.** `modules()` in `symbols.rs` is the one place that knows the full module
  set, so a new subsystem is one line there plus its own declaration. A test asserts that no
  symbol is declared twice and that every module contributes at least one, because a
  subsystem in the workspace but not wired in here is invisible in every shim at once.
