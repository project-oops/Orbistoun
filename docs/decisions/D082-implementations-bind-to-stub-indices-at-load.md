# D082 - Implementations bind to stub indices at load

**Status:** decided
**Date:** 2026-08-19

At load, each import's implementation is looked up and placed in a handler table indexed by
stub. On a call, a present handler is called and an absent one is recorded and refused; both go
through one dispatch path, so the trace records implemented calls too.

**Why:** a registry the call path never consults makes implementing a function change nothing.
Losing visibility of a call the moment it works would hide the traffic worth understanding.

**Rejected:**
- Two kinds of stub, implemented and recording: implemented calls vanish from the trace.
- Resolving at call time: a lookup on every guest call.
