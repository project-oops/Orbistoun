# D451 - A guest's arena-scoped allocator shares this project's one heap, the arena handle unused

**Status:** assumed
**Date:** 2026-09-01

The platform's arena-scoped allocator family answers a guest's allocation and release requests
from the single heap this project already owns, ignoring the arena handle a guest passes.

**Why:** A guest touches memory from this family only through the same family's own calls, so an
allocation and its later release agree with each other regardless of whether the arena handle
names a real, distinct heap. A guest that creates its own arena over a specific address range and
then checks that an allocation came from it is not modelled.

**Rejected:** modelling a distinct heap per arena handle - unneeded complexity with nothing
observed depending on arena isolation.
