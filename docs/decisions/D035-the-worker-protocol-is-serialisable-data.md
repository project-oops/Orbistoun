# D035 - The worker protocol is serialisable data

**Status:** decided
**Date:** 2026-08-19

Shim and worker messages are serde types defined apart from their transport, and service
operations take and return serialisable values.

**Why:** the transport can change without the protocol moving. An operation handing back rich
types that reference a loaded module cannot cross a process boundary.

**Rejected:**
- Protocol defined by the transport: a transport change becomes a protocol change.
- Service operations returning references: unusable from the worker.
