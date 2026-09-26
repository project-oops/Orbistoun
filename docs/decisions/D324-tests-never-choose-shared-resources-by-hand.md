# D324 - Tests never choose shared resources by hand

**Status:** decided
**Date:** 2026-09-26

A test reserving host memory takes its address from `orbistoun-mem::test_bases`, a table of
per-crate ranges asserted distinct, which ships outside `cfg(test)`. Process-wide state that
is incidental to a test is passed in rather than reached for; where the shared state is what
is under test, the tests that touch it are serialised behind an exclusive guard.

**Why:** tests run in parallel, within and across binaries, so two picking the same address
race and fail intermittently. A per-binary counter cannot see other binaries, and a
`cfg(test)` table is invisible to dependents, which would each define their own. A convention
in prose was broken while it was being followed.

**Rejected:**
- Picking a gap by reading the file: the collision it prevents recurs.
- Per-crate test-only tables: two crates choose the same range.
- Passing state that is itself under test: there is nothing to pass.
