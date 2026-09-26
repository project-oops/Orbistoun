# D473 - A C++ throw with no unwinder is a named stop, not a return

**Status:** assumed
**Date:** 2026-09-02

Every guest entry point whose sole purpose is to raise a C++ exception reports which
exception was thrown, with its message, and ends the run there, rather than returning to the
caller.

**Why:** the caller's code was generated on the promise that the call does not return, so
returning would continue execution at a point the guest's own compiler proved unreachable.
Stopping cannot deliver the exception to a handler the guest may have had, but it is a
legible gap rather than a silent wrong continuation.

**Rejected:**
- Returning a placeholder and letting the guest continue: keeps the guest alive but resumes
  it at a point proven unreachable, with no visible relationship between the throw and the
  eventual failure.
