# D472 - Missing imports are triaged by where their semantics come from

**Status:** decided
**Date:** 2026-09-02

An unimplemented import is classified as documented (a published specification, written in
bulk with no guest needed), a title's own module (loaded rather than reimplemented), or
vendor-only (unpublished, so the guest is the only oracle); only the last is chased one wall
at a time.

**Why:** the import list is knowable before a guest ever runs, since interception is linking
rather than hooking, so the documented gap can be closed mechanically instead of waiting to
be discovered one crash at a time. A title's own module is not owed a reimplementation at
all.

**Rejected:**
- Treating every missing import the same way, found only by running guests until they
  crash: correct for a vendor-only function, wasteful for a documented one, and wrong for a
  title's own module, which should be loaded rather than reimplemented.
