# D366 - Name resolution answers with the import stub

**Status:** decided
**Date:** 2026-08-29

The thunk table has a second population after the guest's imports: one stub per implemented
name, in one stable list, with a name-to-address map. A by-name lookup answers the address the
linker would have written for that name. `len()` stays the guest's import count and `total()`
covers both; an unlabelled stub reads `unknown#<index>`. Every distinct name asked for is
reported once.

**Why:** one address per function means one counter, one trace entry and one implementation
however the guest reached it. Dispatch is indexed by one number, so a second table would need
a second trampoline. Reports must keep meaning the guest's imports, while diagnostics are sized
by `total()` so by-name calls are not silently excluded. A resolution pass is the clearest
statement of what a payload needs.

**Rejected:**
- A resolver minting its own answers: a second behaviour per call.
- A separate table for by-name stubs: a second trampoline and counter array.
