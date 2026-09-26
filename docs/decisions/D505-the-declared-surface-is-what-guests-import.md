# D505 - The declared surface is what guests import

**Status:** decided
**Date:** 2026-09-03

Every library a guest imports is declared, whether or not anything in it is implemented, in the
crate where its implementation will live. A library that declares names and implements none is
listed in `SERVES_NOTHING` with its reason, and `status` counts those names separately.

**Why:** a declared surface that is nearly the implemented set makes coverage look finished by
construction. Declaring what guests ask for makes the declared/implemented ratio measure
something, and a guest reaching an unimplemented interface is named and counted rather than
answered by a placeholder it might store as a handle.

**Rejected:**
- Declaring only what is implemented: the coverage figure can only ever look complete.
- Implementing part of a library to shorten `SERVES_NOTHING`: writes the abstraction before its caller.
- Placing declarations wherever is convenient now: the implementation would later have to move.
