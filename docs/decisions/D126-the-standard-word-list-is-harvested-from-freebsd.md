# D126 - The standard word list is harvested from FreeBSD

**Status:** decided
**Date:** 2026-09-26

The published-standard word list is generated from every `.map` version script in a cited
FreeBSD revision, recorded in the file's header. Names with a leading underscore are kept;
`FBSDprivate_*` blocks are skipped because FreeBSD marks them private. A malformed map costs its
own symbols, not the run.

**Why:** a list written from memory cannot be audited and is bounded by what one person
recalled. Where a source states a distinction, the distinction is taken from it: a rule invented
on top - skipping reserved names, or reading only files called `Symbol.map` - silently drops
real interface names.

**Rejected:**
- A hand-written list: unauditable.
- Skipping reserved names: loses the C++ ABI.
- Matching `Symbol.map` only: loses the threading library.
