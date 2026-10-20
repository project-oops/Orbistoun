# D904 - A dispatch samples through its guest sampler

**Status:** decided
**Date:** 2026-10-19

A compute program's image sample reads through the sampler its program names, the same as a draw's
(D690): filter, address modes and border are decoded from the sampler words, and a Vulkan sampler is
cached per device and sampling. A fetch ignores the sampler.

A compute stage has no derivatives. A sample there is translated only at a named level (`_lz`, `_l`)
or with explicit gradients (`_d`). A sample whose level would come from derivatives is refused by
name. A table border colour is refused as it is for draws.

**Why:** Grand Theft Auto V's compute programs sample their inputs with `image_sample_lz`. With no
sampler bound the dispatch could not be built.

**Rejected:**
- One nearest sampler for every dispatch: it drops the linear filtering the programs ask for.
- Treating an implicit-level sample in compute as level 0 silently: the hardware takes its
  derivatives from the quad, and the guess is not written down anywhere.
