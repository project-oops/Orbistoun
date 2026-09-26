# D004 - Crates on a dependency spine

**Status:** decided
**Date:** 2026-09-26

The core crates form one dependency spine - `core`, `elf`, `nid`, `mem`, `hle`, `loader` - and
every subsystem crate sits above it. A crate nothing depends on is deleted rather than kept for
later.

**Why:** a subsystem is never reached until a guest has loaded, mapped and run, so the order of
the spine is the order in which code can be exercised. Code written ahead of that order cannot
be tested and cannot be trusted. The spine is the first description of the workspace a reader
meets, so a crate in it must be real.

**Rejected:**
- A single crate: no boundary stops a subsystem reaching into another.
- Speculative crates kept for future use: they describe capability that does not exist.
