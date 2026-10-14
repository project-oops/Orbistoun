# D855 - A dispatch's partial-group shape is read at run time, not compiled in

**Status:** assumed
**Date:** 2026-10-14

A dispatch with partial groups (`PARTIAL_TG_EN`) carries its last group's index and the threads it
runs along each dimension in the push-constant block, at `PARTIAL_PUSH_WORD` - the six words after
the thirty-two a stage's user data may take - and its module reads them there. A thread runs
unless, along a dimension, its group is the last and its id is at or past that dimension's count.
The module's name in the translation cache says only whether the dispatch has partial groups, so
every grid a program is dispatched over shares one module, one translation and one pipeline.
A dispatch without partial groups keeps the mask compiled in, as before.

**Why:** the shape was compiled into the mask, so each grid size was another module: SuperTuxKart
dispatches its programs over every texture size it has, and 12 programs became 173 modules in 25
seconds, each a Vulkan compute pipeline to build - up to a second apiece, a third of its run. With
the shape read at run time, the dispatch and compute-pipeline zones went from about 8.5 s of a 30 s
run to about 1.5 s once the driver's cache was warm, and its median frame from 590 to 230 ms.

**Rejected:**
- Keeping the compiled-in mask and caching modules by shape: the shapes are as many as the guest's
  grid sizes.
- Masking the partial threads on the host by dispatching fewer groups: a group's threads are one
  wave, and a partial one still runs as a group the program counts.
