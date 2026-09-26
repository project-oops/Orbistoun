# D207 - orbistoun implements obSCEne's protocol

**Status:** decided
**Date:** 2026-09-26

obSCEne owns the probe protocol and its record format; orbistoun implements them as a client that
drives sessions and records the operator's assertion of machine identity, and as a responder
that announces only what it serves and identifies itself as an emulator. Unknown commands are
refused. No code is shared. Tests and CI never open a socket.

**Why:** the protocol asks about the platform, not about either program, so obSCEne stays useful
to someone with no interest in this emulator. Shared code would couple the builds. A
transcript of orbistoun's own answers is graded no higher than `assumed`, or the project marks
its own homework.

**Rejected:**
- orbistoun shaping the protocol: the probe becomes this emulator's harness.
- A shared parser: a build-time dependency in both directions.
- Improvising an answer to an unknown command: plausible and wrong.
