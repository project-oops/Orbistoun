# D428 - Loading a system module always succeeds

**Status:** decided
**Date:** 2026-09-01

`sceSysmoduleLoadModule`, `UnloadModule` and `IsLoaded` all answer success for any module
identifier a title names.

**Why:** Every library a title imports is already resolved by the loader before the guest runs,
so by the time a guest asks whether a system module is loaded, it necessarily is. Answering the
kernel's generic placeholder here left a guest holding a value it stores as a module handle and
dereferences later, the same shape as an unchecked null pointer.

**Rejected:** tracking a real load state per module identifier - no lawful source documents which
identifiers exist or their load order, and nothing observed needs the distinction.
