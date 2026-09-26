# D131 - A capture pairs a call with its bytes

**Status:** assumed
**Date:** 2026-09-26

A command-stream capture under `crates/orbistoun-gpu/tests/captures/` is a pair: what the
library call asked for, and the bytes it appended. The vocabulary test checks each expectation
against a decode of the bytes. An empty corpus reports rather than passes.

**Why:** a recorded buffer alone would be read through the packet table under test, so agreement
would prove nothing. A call's arguments state the answer while its bytes are the question, which
makes the pair an oracle rather than a mirror.

**Rejected:**
- Captured command buffers alone: the table checks itself.
