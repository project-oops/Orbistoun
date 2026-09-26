# D376 - Runtime-global resolution by guest symbol name

**Status:** assumed
**Date:** 2026-08-29

When a run enters a guest past its own startup code, the loader resolves that
runtime's unfilled global library pointers by name, from the guest's own symbol
table, and marks every entry it cannot resolve rather than leaving it null.

**Why:** Entering past the startup code skips the resolution that code would
have performed, leaving every runtime global null and faulting on the first use
in a way that measures the skip rather than the guest. Performing the same
by-name resolution, and marking what is left, keeps the run legible: a marker
names the next boundary instead of a null pointer with no meaning.

**Rejected:**
- Leaving all globals null: indistinguishable from a real gap, and hides which
  globals a normal boot would have filled.
- Resolving every global regardless of ambiguity: several are unrelated statics
  that share one library function's name, and filling them writes pointers into
  memory that has nothing to do with resolution.
