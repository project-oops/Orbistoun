# D483 - An import binds by library identity and hash together

**Status:** decided
**Date:** 2026-09-03

An encoded import binds only into the module its own library identifier names, never into a
different module that happens to export a symbol with the same hash. Where both orbistoun's
own implementation and a title's shipped module could answer an import, orbistoun's
implementation wins.

**Why:** a name hash alone collides across modules by construction, so matching on the hash
without the library identifier can bind an import into the wrong module. Preferring
orbistoun's own implementation keeps every existing measurement valid while still resolving
imports a title's own modules would otherwise leave unbound.

**Rejected:**
- Matching by hash alone, falling back across every module: binds an import into a module it
  never named, indistinguishable from a bug until traced.
- Preferring a title's own shipped module wherever one exists: matches the platform's real
  behavior, but unsafe until its modules are fully relocated and bound.
