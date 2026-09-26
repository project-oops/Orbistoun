# D676 - Unanswered weak imports bind to zero

**Status:** decided
**Date:** 2026-09-10

An import marked `STB_WEAK` that no loaded module, the title or the platform implementations
answer binds to address zero, plus the addend for an absolute relocation, and does not block
entry. A strong import nothing answers still gets a callable stub, and the loader follows the
binding the binary declares.

**Why:** the ELF gABI resolves an unsatisfied weak reference to zero and lets the link succeed,
and C feature detection tests exactly that address against null. A stub makes every such test
read present and enters paths the platform does not provide; refusing entry fails a link the
platform's own linker accepts.

**Rejected:**
- A callable stub for every unresolved import: every weak feature check reads present.
- Refusing to enter the image: stricter than the platform's linker.
- Inferring weak binding a packager erased: the binary's metadata is what the loader has.
