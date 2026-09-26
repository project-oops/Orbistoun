# D640 - Imports bind to title-shipped modules

**Status:** decided
**Date:** 2026-09-26

An import that a module the title ships exports is bound to that export, for the executable and
for every sibling module, unless orbistoun implements the library, in which case orbistoun
answers. Every run prints an account of every import, bound or not, grouped by reason and library.

**Why:** a module only the game could have written has no reimplementation to offer, so answering
its exports with a placeholder leaves the guest asking forever. A library orbistoun implements,
such as a title's own copy of the C library, is intercepted as every import is. The account makes
"bound" and "silently unbound" distinguishable, and each unbound reason points at a different fix.

**Rejected:**
- Always intercepting: the title's own code becomes unreachable behind placeholders.
- Always preferring the title's module: a shipped C library would replace orbistoun's implementation.
- Printing the account only on failure: silence and "nothing went wrong" read the same.
