# D732 - A draw's buffers are bound as device buffers

**Status:** decided
**Date:** 2026-09-27

A draw reads each buffer its shaders name through a device buffer bound to it, as textures are.
The translator traces every buffer descriptor and scalar-load base a shader reads through back to
its user data, constants and descriptor-table loads, and gives each distinct one a slot; the
frontend resolves each slot per draw from the draw's user data and guest memory to a guest range
and its bytes; the backend binds them in a descriptor set of its own, beside the pipeline's. An
access nothing traces still reaches the window (D711), and a draw with one and no window is
refused by name. A store through a slot, and a descriptor whose range or addressing the draw cannot
bind exactly, is refused by name. Amends D703 and D711 for draws: the window is no longer the only
guest memory a draw's module reads.

**Why:** radeonsi forms no constant base (D711) and reads its constants, descriptors and vertices
through descriptors in user data and descriptor tables, across many buffers a single window cannot
span. Binding the ranges the descriptors name reads exactly the guest's bytes, with the bounds
rules left to the module, which applies them to the descriptor it read. A set of its own lets a
cached pipeline serve every draw whose buffers differ, so pipelines key on layout.

**Rejected:**
- A window per draw: one window still cannot span buffers far apart, and moving it recompiles
  every module (the base is compiled in).
- Buffers in the pipeline's own set: every distinct buffer would build another pipeline.
- Physical storage buffer addresses: host device addresses are not guest addresses, so every
  pointer still needs mapping, and a range with no mapping cannot be refused.
