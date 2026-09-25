# D718 - consecutive guest draws that share their state are one host mesh dispatch

**Status:** decided
**Date:** 2026-09-25

## The question

A GL guest issues one draw per triangle: about 17k a Neverball frame, ~340 per submission. Each became
a host mesh dispatch with its own bind, push constants and `vkCmdDrawMeshTasks(1)`, about 1 us of host
work apiece on the device thread (worklog 852). On the console the command processor walks those
packets in hardware. How can the host do the same work in fewer commands without changing what is
drawn?

## The choice

**A run of guest draws that differ only in their geometry stage's user data is one mesh dispatch, of
one workgroup per draw.**

- A mesh module reads its user data from a storage buffer at binding 5, at `WorkgroupId.x * 16`,
  rather than from the push-constant block every workgroup shares. Other stages keep the push block.
- The backend holds an open batch in the resident pass, keyed by:
  - the cached pipeline, which is its shaders, textures, blend, viewport and scissor;
  - the attachment;
  - the window buffer and slot;
  - the fragment stage's words.

  A draw with the same key appends its geometry words and records nothing else.
- Anything else recorded into the pass records the batch first: a different draw, a non-mesh draw,
  closing the pass. So everything keeps the order the guest gave it.
- Draw data is written into one mapped host-visible buffer as each batch is recorded, and taken
  back when `settle` leaves the device idle.

## Why it is exact

The Vulkan specification's mesh shader primitive ordering: "All output primitives generated from a
given mesh workgroup are passed to subsequent pipeline stages before any output primitives generated
from subsequent input workgroups". So workgroups 0..N-1 rasterise and blend in the order the N draws
would have. Each workgroup reads exactly the words its draw had.

gl1-probe gives the same 110 verdicts, and every sampled pixel identical, with and without it.

## Consequences

- Across 30 dumped Neverball submissions, the runs average 234 draws and never a single draw.
- A mesh module whose user data is not the geometry stage's share (block offset 0) is refused by
  name rather than read from the wrong words.
