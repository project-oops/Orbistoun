# D688 - A primitive shader translates to a mesh shader

**Status:** assumed
**Date:** 2026-09-14

A next-generation-geometry primitive shader, whose one wave declares its vertex and primitive
counts with an allocation message and then exports primitives and vertices, translates to a host
mesh shader. The allocation message's `m0` carries vertices in its low twelve bits and
primitives above them.

**Why:** a mesh shader's output declaration, primitive indices and per-vertex writes correspond
one to one with the allocation message and the primitive and vertex exports. Primitive shaders
exist to cull, so a real one exports fewer primitives than it was given with a count computed at
run time; a vertex-shader translation would silently draw primitives the guest culled.

**Rejected:**
- A vertex shader dropping the primitive half: correct only for an identity primitive export,
  silently wrong otherwise.
- Pattern-matching the identity case: fits one shader and breaks on the next that differs by a
  register.
- Compute writing vertices for a second draw: kept as the fallback where the mesh extension is
  unavailable.
