# D701 - Resources reach the backend by residency

**Status:** decided
**Date:** 2026-09-16

The frontend reads guest memory, content-hashes each resource and hands the backend plain data
through `ensure_resident`, idempotent per `ResourceId` and called before the commands that name
it. `Resource` grows one variant per kind, and the backend owns and evicts its host objects.

**Why:** shaders, buffers and textures reach the backend the same way, so the mechanism is
residency, not shader delivery. Content addressing makes invalidation automatic: a rewritten
resource has new bytes and a new id. Reading guest memory on the frontend keeps the backend
clear of the guest address space and `orbistoun-gpu` clear of any graphics API.

**Rejected:**
- A shader-only load call: rebuilt for every later kind.
- Handing the backend the whole submission: moves frame sequencing into every backend.
- Bytes inside the commands: resent on every bind, defeating the content cache.
- A guest-memory handle for the backend: puts the guest address space behind the API seam.
