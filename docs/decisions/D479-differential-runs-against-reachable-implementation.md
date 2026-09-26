# D479 - A differential runs against whichever reachable implementation is published

**Status:** decided
**Date:** 2026-09-02

The differential check runs against any reachable, published implementation of the same
interface, naming which one it used, rather than requiring the target's own documented
ancestor specifically.

**Why:** the nearest lawful analogue is not always available to provision locally, and a
differential record already has to name its source. Using a reachable implementation and
citing it satisfies that obligation without blocking the check on an environment that cannot
be provisioned.

**Rejected:**
- Requiring the exact documented ancestor implementation: correct in principle, but
  unavailable without an elevated environment or a large third-party download, which blocks
  the check entirely.
