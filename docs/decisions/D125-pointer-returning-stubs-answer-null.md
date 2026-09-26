# D125 - Pointer-returning stubs answer null

**Status:** decided
**Date:** 2026-08-20

The knowledge file records what each function returns - status, pointer, handle or count - and
an unimplemented function returning a pointer, handle or count answers zero rather than an error
code.

**Why:** a guest dereferences what a pointer-returning call hands back, so an error code becomes
a wild pointer that faults somewhere unrelated. Null is what real allocators and resolvers
return on failure, guests already check for it, and a null dereference faults at a recognisable
address.

**Rejected:**
- An error code for every unimplemented function: right for status, dangerous for pointers.
