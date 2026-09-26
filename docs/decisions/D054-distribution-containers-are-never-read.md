# D054 - Distribution containers are never read

**Status:** decided
**Date:** 2026-08-19

orbistoun never reads a package or a disc image. It accepts the installed directory tree a
guest sees at `/app0`, and mounts are ordered layers in which a later layer shadows an earlier
one.

**Why:** distribution containers are encrypted and need keys, which the repository never holds
(D014). Both formats converge on the same directory tree before a guest runs. A patch or
overlay shadows base files, so a single root would emulate a version nobody runs.

**Rejected:**
- Parsing packages: requires keys.
- A single mount root: loses patch and overlay layering.
