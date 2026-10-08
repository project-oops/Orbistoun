# D758 - A primitive shader's global loads are bound per draw from their lanes' addresses

**Status:** decided
**Date:** 2026-10-08

A primitive shader's `global_load_dword*` with no scalar base is traced as a draw buffer (D733).
Every such load in a program reads one buffer, placed after the program's other buffers. Its source
is the program through the last load, as for a computed descriptor (D753). For each draw that
prefix runs on the host lane by lane, vector instructions included, through the translator's own
semantics. The run starts as the module does: the user data, every execution-mask bit of the wave
set, and every vector register zero but the ids the module seeds in a vertex lane, which are
unknown. It follows each branch whose condition is known.

The draw binds the range from the lowest address an active lane forms to the end of the widest read
from the highest. A lane is active when its execution-mask bit is known set at the load and it is
one of the draw's vertices. The module reads the range relative to its guest base, which the draw's
words carry beside the range's place (D747): one word per slot, the address's low half, because a
flat address is translated from its low half. A load whose lanes' addresses or mask the host cannot
know is refused by name, as is a draw of more vertices than one wave holds.

The run is kept cheap without changing what it finds. It runs only the vector instructions whose
results can reach a load's address, a backward slice to a fixed point. When no instruction it runs
reads one lane from another, it runs only the draw's vertex lanes. A lane read, a permute, a
data-parallel or local-data-share access, or a read of a vector-written mask other than a lane's
own bit all count as reading another lane. The decoded program and its slice are found once per
program. A run's answer is kept by program, user data and vertex count while every word it read
still reads the same.

**Why:** the open-toolchain GL context's GLSL path (oops-sdk `glsl_vs.c`, `vs_load_attributes`)
fetches each vertex attribute with `global_load_dword*` from `base + stride * index`. Its base and
stride are words it loads from an attribute table, and the index is `s12` plus the lane. The window
(D711) sits wherever the shader's constant base is, and a vertex buffer is wherever the context
allocated it. TSHP00001's first vertex buffer is 4 MiB past its window, and a later one 2.8 GiB past
it. Every position read zero, and no triangle of either Zelda port reached a pixel. A global load
has no descriptor, so nothing but the addresses the lanes form bounds it. Each of TSHP00001's
thousands of draws a second brings its own base vertex, so the run is per draw and must be fast.

**Rejected:**
- Binding the whole mapping from the traced base: it is the context's arena, far more than a draw
  binds.
- Tracing the address as an expression of the table's words and the lane: it restates the
  instruction semantics the translator already has (D753's reason).
- Assuming the address grows with the lane, so two lanes bound it: an assumption the per-lane run
  does not need.
- One buffer per load: the context fetches a four-component attribute through two loads either side
  of a branch, and five attributes would pass the eight buffers a stage binds.
