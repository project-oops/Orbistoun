# D325 - A memory fill reports what it filled

**Status:** decided
**Date:** 2026-09-26

The stack, heap, direct-memory and `.bss` fills are opt-in diagnostics, each reporting how many
regions and bytes it filled, and a fill asked for that never fired says "nothing was tested".
Fills touch writable mappings only. The `.bss` fill writes markers whose low bytes are the
slot's own address, and an unparseable fill value leaves the fill off.

**Why:** a poison that changed nothing and one that never ran produce identical output, and
recording an elimination on that evidence stops anyone looking. Writing a read-only mapping
would fault inside the emulator and read as the guest's fault. A marker naming its own address
turns a use of an unset global into a fault that names it. Zeroed `.bss` is the C guarantee,
so a typo must never turn a diagnostic on.

**Rejected:**
- Silent fills: an elimination indistinguishable from an untested class.
- A constant `.bss` fill: shows that a global was used, not which.
