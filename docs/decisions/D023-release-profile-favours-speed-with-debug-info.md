# D023 - Release profile favours speed, with debug info

**Status:** decided
**Date:** 2026-08-19

Release builds use `opt-level = 3`, thin LTO, one codegen unit and `debug = 1`, unstripped.

**Why:** the emulator is on the hot path of every guest call. Line tables stay because a guest
crash is unreadable without them, and their cost is paid once on disk.

**Rejected:**
- A size-optimised profile: the wrong trade for a hot-path emulator.
- Stripped binaries: fault reports lose their host frames.
