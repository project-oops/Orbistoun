# D371 - Files and sockets share one descriptor table

**Status:** decided
**Date:** 2026-08-29

Files and sockets are entries in one descriptor table, so `read`, `write` and `close` take either.
A socket is pending until `listen` or `connect` makes it a host object, remembering what `bind`
was told. `setsockopt` succeeds and applies nothing, and says so in the knowledge file. Socket
addresses follow the FreeBSD layout with a length byte first.

**Why:** a guest has one numbering space, and two tables make a descriptor mean different things
to different calls. The host creates a listener in one step, so binding early would hold the port
twice. Failing `setsockopt` stops a correct server before `bind`, while an unverifiable option
mapping changes socket behaviour silently. The address layout was read from the headers, not
recalled.

**Rejected:**
- Separate file and socket tables: two numbering spaces.
- Binding at `bind`: the port is held twice.
- Refusing `setsockopt`: ends every server before it listens.
