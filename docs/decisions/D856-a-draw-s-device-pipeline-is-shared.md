# D856 - A draw's device pipeline is shared across the textures it samples

**Status:** assumed
**Date:** 2026-10-14

A draw's pipeline is kept by a key over its shaders, its fixed state and its textures (D744's
in-run cache). Its shape - the same key without the textures and the user clip planes (D857) -
names the device pipeline, its layouts and its shader modules, which depend on neither. The first
build of a shape keeps those for the process, up to 2048 shapes; every later build of the same
shape uses them and makes only its own textures and descriptor set. Releasing a pipeline releases
what it made and leaves a kept shape alone. A draw still batches only with draws of its whole key
(D718), so draws sampling different textures never share a set.

**Why:** a title whose textures change from draw to draw built a whole pipeline each time:
SuperTuxKart and Neverball created a device pipeline for nearly every draw - 3,073 and 1,150 in
30 seconds - though they drew with a few dozen shapes. Even found in the driver's cache, a
translated module's pipeline takes milliseconds to create. Sharing the shape took SuperTuxKart's
creations to 143 and Neverball's (with D857) to 45.

**Rejected:**
- Keeping textures out of the pipeline entirely, cached by content and bound through a set of their
  own: the end state, and a larger change to the layout, the translator's bindings and the batch;
  this keeps the pipeline object as it is and shares what is shareable.
- Releasing kept shapes with the in-run cache's drain: a pipeline taken out of the cache for a draw
  would be left holding a destroyed device pipeline.
