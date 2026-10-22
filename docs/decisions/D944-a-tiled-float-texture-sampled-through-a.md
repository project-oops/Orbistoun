# D944 - A tiled float texture sampled through a compare is read by its swizzle equation

**Status:** decided
**Date:** 2026-10-21
**known_by:** published

A module may sample a `32_FLOAT` image through a compare sampler where the image is not a depth
target (D817): a depth texture the title wrote itself. A linear one is read row by row, as before.
One in a tiled swizzle mode is detiled by that mode's equation (D923), as an untargeted depth
surface is (D939), and bound as compared depths. A chain, an array, a cube or a volume is refused as
before.

**Why:** Grand Theft Auto V compares against a 1x1 `32_FLOAT` texture in `256B_S`, and the reader
took only linear ones, so the draw was refused and every later submission waited behind it.
