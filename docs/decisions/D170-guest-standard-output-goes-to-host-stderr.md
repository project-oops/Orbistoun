# D170 - Guest standard output goes to host stderr

**Status:** decided
**Date:** 2026-09-26

A guest's writes to descriptors 1 and 2, and its `printf`-family output, land on the host's
error stream, never its output stream.

**Why:** a conformance probe that cannot write to standard output cannot report at all, and a
guest's own diagnostics name what failed. The worker's standard output carries the protocol as
newline-delimited JSON, and guest bytes interleaved into it would break the reader.

**Rejected:**
- Refusing descriptors 1 and 2: a guest that cannot talk.
- Host standard output: corrupts the worker protocol.
