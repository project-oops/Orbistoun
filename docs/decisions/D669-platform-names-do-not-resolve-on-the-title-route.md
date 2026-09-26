# D669 - Platform names do not resolve on the title route

**Status:** decided
**Date:** 2026-09-10

A run's route is decided by the loader: a module with `sce_sys/param.json` or
`sce_sys/param.sfo` beside it runs as a title, anything else as a payload. On the title route a
lookup by name into the platform's own libraries is refused with the platform's measured module
error, and names the guest's own modules export still resolve.

**Why:** the hardware does not resolve platform names for a launched title, and a lookup that
answers makes a guest classify its run as a payload and compare itself against the wrong
conformance leg. The refusal is a platform rule, not an emulator gap, so the unimplemented
placeholder would misreport it. Titles bind their imports and only probe by name, so the refusal
moves no title's reach.

**Rejected:**
- Resolving every name on every route: answers what the hardware refuses and misclassifies the
  run.
- Deciding the route from the executable: one file can be byte-identical on both routes.
- Answering the unimplemented placeholder: reports a platform rule as a missing implementation.
