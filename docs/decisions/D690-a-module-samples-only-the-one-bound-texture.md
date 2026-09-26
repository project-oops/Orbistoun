# D690 - A module samples only the one bound texture

**Status:** assumed
**Date:** 2026-09-15

Every image sample in a translated module reads the one sampled image the pipeline bound. A
sample naming a different descriptor register range from the first, or following a write into a
recorded range, is refused at translation.

**Why:** mapping every sample onto whatever is bound renders a frame that looks right and is
not. Resolving a descriptor properly needs a decoder, a surface cache, a format table and an
upload path, a subsystem rather than a prerequisite for one instruction. Register identity with
write invalidation is the strongest claim available without decoding, and its false refusals err
on the safe side.

**Rejected:**
- Mapping every sample to the bound texture: plausible wrong frames.
- Decoding the descriptor first: a subsystem built blind.
- Counting distinct textures: needs the decoder that does not exist.
