# D032 - The guest executes in a child process

**decided** · 2026-08-19

Not in the GUI or CLI process. The decisive argument is **address space**.

The guest demands fixed addresses - `MAP_FIXED_NOREPLACE` at the exact bases a module
was linked for, failing rather than relocating (D016). In a shim's process those
addresses compete with the UI toolkit's allocations, the graphics driver's mappings,
every loaded DLL, and ASLR. A conflict means the guest cannot load *at all*, and it
would be **nondeterministic** - varying by driver version, by allocation order, by
run. That is the worst possible failure class for a project whose early value is
trustworthy diagnostics. A child process offers a nearly-empty address space.

Three arguments compound it:

- **Thread reclamation.** The guest creates real host threads and a thread cannot be
  safely killed mid-execution. In-process, threads the guest never joins persist for
  the life of the UI. Process exit reclaims them exactly.
- **The dev loop is load → crash → tweak policy → reload.** In-process reload means
  unmapping guest memory and resetting global state while guest threads may hold
  locks in arbitrary states - a known source of state-leak bugs. Process teardown is
  exact and free.
- **Fault handlers.** Catching guest access violations in a shared process means
  competing with the toolkit, the driver, and the runtime over the same handlers. In
  a dedicated process the policy is ours alone.

**Cost, stated honestly:** video output is produced in the child while the window
lives in the shim, so phase 6 needs either a reparented child-owned window or shared
images via external-memory extensions. Deferring that is legitimate rather than a
dodge - until phase 6 the child produces no video at all, only traces and
diagnostics, and the control/event channel is needed either way.

Supersedes an initial in-process lean, which was a shortcut under D028.

