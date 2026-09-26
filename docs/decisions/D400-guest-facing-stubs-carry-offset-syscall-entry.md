# D400 - Every guest-facing stub carries a syscall entry at a fixed offset

**Status:** decided
**Date:** 2026-08-30

Every address this project hands a guest as an implementation carries a
working syscall dispatch entry reachable by jumping into it partway through
its body, at any of several small offsets, rather than only at its start.

**Why:** An open-toolchain guest resolves an ordinary function and computes
its syscall entry point as a fixed offset into that function's own body,
expecting to land inside a real instruction on the platform. Every address
this project hands out is otherwise a self-contained stub with nothing at
that offset, so the guest's own convention needs somewhere real to land
regardless of which library build produced the offset it uses.

**Rejected:**
- Serving the syscall entry only at a stub's starting address: matches a
  guest that calls normally and nothing about a guest that computes an
  internal offset.
- Supporting exactly one fixed offset: works for one library build and
  silently misleads guests built against a different one.
