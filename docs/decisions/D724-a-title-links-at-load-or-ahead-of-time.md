# D724 - A title links at load or ahead of time from one link plan

**Status:** assumed
**Date:** 2026-09-26

The loader produces a link plan: the placed segments, the thunk index each import resolves
to, the relocations, the thread-local storage layout, the instruction rewrites (D725) and the
raw `syscall` sites. Handlers and stub answers attach to thunk indices when the guest starts,
so the plan holds no implementation. It is stored in the title library, keyed by the
executable's hash, the loader's build and the host CPU's features, and every run applies the
stored plan when its key matches and builds one when it does not. There is no option: a
stored plan is a cache. A native executable is the same plan written as a host image that
loads one orbistoun runtime library, built only by `orbistoun-cli link --native` and never by
`run`. Every mode keeps the thunks, so every mode writes the same trace.

**Why:** native execution means the guest runs the same way in every mode; only the time of
linking differs, so which one ran is not a property of the title. Keying on what the plan was
computed from, rather than on what implements it, lets an implementation land without a
relink. Keeping the thunks keeps the trace, and with it the loop.

**Rejected:**
- A per-title override choosing the mode: the mode says what the operator is doing, not how
  the title behaves, and verdicts would compare runs made in different modes.
- A launch option choosing the mode: a stored plan that differs from a fresh one is a
  loader defect to report, not a preference.
- A separate ahead-of-time relinker beside the loader: two linkers disagree, and each
  disagreement reads as a guest fault.
- Native output that binds imports straight to the runtime library: loses the per-import
  count and order the trace depends on (D062).
