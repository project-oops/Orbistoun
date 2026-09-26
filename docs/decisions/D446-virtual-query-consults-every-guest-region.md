# D446 - A virtual-address query consults every region a guest can legitimately read, not only the runtime map

**Status:** decided
**Date:** 2026-09-01

`sceKernelVirtualQuery` looks an address up across the runtime allocation map, the loaded image,
the calling thread's own stack, and the main stack span, in that order, instead of only the
runtime map.

**Why:** The loaded image, a thread's stack and the main stack are set up by the loader and
worker rather than allocated at guest run time, so the runtime map alone never sees them; a guest
querying its own code or stack address needs an answer regardless of which of these owns it.

**Rejected:** teaching the runtime map to also record these regions as if they were guest
allocations - conflates two different kinds of region and complicates every other consumer of the
map.
