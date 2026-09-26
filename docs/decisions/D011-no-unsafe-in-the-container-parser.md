# D011 - No unsafe in the container parser

**Status:** decided
**Date:** 2026-08-19

The container parser contains no `unsafe`; every structure read goes through `zerocopy`, which
validates size and alignment first.

**Why:** the parser reads hostile bytes, which is the last place for hand-written pointer casts.

**Rejected:**
- Pointer casts over the input buffer: an out-of-bounds read on a malformed file.
