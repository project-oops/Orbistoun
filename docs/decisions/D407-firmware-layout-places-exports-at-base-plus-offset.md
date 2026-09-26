# D407 - A presented firmware places exports at base plus measured offset

**Status:** decided
**Date:** 2026-08-30

When a run presents a firmware, the firmware region places each library
export this project has a measured offset for at a fixed base address plus
that offset, and the guest's initial resolver handle is the address of one
specific, always-present export placed the same way; a run presenting no
firmware keeps the existing resolver-based handoff unchanged.

**Why:** An open-toolchain guest's own startup code computes its library base
from one known export's address and then reaches every other export at
base-plus-offset, which is a scheme this project can only satisfy by actually
placing thunks at those computed addresses. Because those offsets can sit
close together, a placed thunk must fit the real spacing or it overwrites its
neighbor.

**Rejected:**
- Keeping the resolver-only handoff for every run: correct for a guest that
  resolves by name, and unusable for one that computes a base and offsets
  instead.
