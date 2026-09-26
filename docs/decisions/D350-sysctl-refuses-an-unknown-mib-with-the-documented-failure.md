# D350 - sysctl refuses an unknown MIB with the documented failure

**Status:** decided
**Date:** 2026-08-27

`sysctl` answers the documented failure for a MIB it does not know and reports each distinct
unknown MIB once. `getpid` answers the host process id.

**Why:** the manual documents that failure for an unknown name, so it is a real answer the
caller branches on. Success with no length written hands a size query an uninitialised size.
An unknown MIB is a work item only the guest can name. The guest runs in the host process, so
that id is true from inside it.

**Rejected:**
- Answering success: an uninitialised size to allocate against.
- An invented process id: a value with no meaning.
