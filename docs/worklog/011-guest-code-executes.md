# 2026-08-19 - Guest code executes


Phase 4 finished. In order: segment protection, thread-local layout, per-import stubs,
guest stack, entry jump, postmortem. 22 crates became 23; 186 tests became 230;
D060-D064.

**The headline.** All four commercial executables now place, link, protect, build stubs,
and jump to their entry points. They fault there with an access violation - identically,
which is the useful part: a systematic gap, not four separate quirks.

The 96 MB executable: 174,172 relocations, five protection runs, 1,411 import stubs.

### Surprises

- **An image with zero relocations counts as fully linked.** True, and it authorised the
  entry jump, so the worker test suite started killing its own process - a synthetic
  fixture with no dynamic table jumped to a non-executable address. The fix is not to
  special-case empty tallies but to check the thing that actually matters: whether the
  entry point lies in an executable segment. Better diagnostics for real titles too.
- **A test suite that crashes the harness reads as a flake.** The first symptom was one
  unrelated test in another crate failing intermittently. It passed in isolation every
  time. It was not flaky at all - a *different* test binary was dying, and the runner
  attributed the wreckage to whatever was in flight.
- **Windows renders exit codes in hex.** A test asserting an unknown status "contains 42"
  failed against `0x0000002a`. Test expectation wrong, not code - the third time this
  session that a test encoded an assumption rather than a contract.
- **`mprotect` on a shared page is a real trap, found by reasoning rather than by
  running.** Protecting segments in a loop would have worked on every file tested here,
  because their segments happen to be page-aligned. It would have failed later, on
  something else, as a fault at an address belonging to neither segment.
- **Zero write-plus-execute segments across every executable examined.** W^X holds
  naturally. Worth having built the counter to find that out rather than assuming
  either way.
- **Prose-heavy heredocs keep breaking the shell.** Noted before, hit again. Write tool
  for Rust files with substantial documentation; heredocs only for short edits.

### Outstanding

The fault has no address yet. That is the next thing worth knowing, and it converts
"faults immediately" into an actual work list.

