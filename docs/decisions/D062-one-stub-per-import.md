# D062 - One stub per import

**Status:** decided
**Date:** 2026-08-19

Every import gets its own small stub carrying its index, and one shared trampoline serves the
whole table.

**Why:** one shared target answers only "did the guest call something unimplemented". A stub
per import answers which, in order, with counts - the input to the whole loop - at a few bytes
per import.

**Rejected:**
- One shared unimplemented target: says nothing about which function.
- A trampoline per import: duplicates the register-saving code.
