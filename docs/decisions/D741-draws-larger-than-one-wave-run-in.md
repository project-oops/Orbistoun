# D741 - Draws larger than one wave run in chunks

**Status:** decided
**Date:** 2026-10-05

A draw whose primitive shader reads the geometry engine's inputs (D730) and has more vertices or
primitives than one wave's threads is split into chunks that one wave holds, as the geometry
engine splits a draw into primitive-shader subgroups. This supersedes D730's rejection of
splitting: two titles' draws now need it.

A list's chunk takes `lanes / 3` whole triangles. A strip's takes an even count, `lanes - 2`
rounded down to even, so each chunk's first triangle has the strip's even winding; consecutive
chunks share two vertices. Each chunk is its own module for its own geometry and an indexed draw
of its vertex count. Its vertex threads read their ids as indices (D740): an indexed draw's own,
from where the chunk's first primitive's begin; a non-indexed draw's as thirty-two-bit ids
counting up from that position, built by the host.

A vertex shared between chunks is computed in each. Every vertex is computed from its own id,
so it is the same vertex.

**Rejected:**
- Refusing the draw: two titles' frames are made of them.
- Wave-sized subgroups chosen by vertex reuse, as the hardware's vertex cache does: the
  partitioning is the hardware's choice, and a correct primitive shader draws the same for any.
