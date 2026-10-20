# D920 - A flat sample of an array reads its first layer

**Status:** decided
**Date:** 2026-10-20

A draw may sample, with `dim:2D`, an image whose descriptor is a 2D array or a cube, its view
starting at the first layer. That sample reads the first layer, or the first face. For a depth
surface, that is the target the host drew at slice zero (D909, D918). For any other image, it is
the first layer's level read as a flat image would be. This is `assumed`; an obSCEne request asks
hardware what such a sample returns.

A `16_UNORM` texture binds as `R16` in draws: sixteen bits a texel, two to a word, sampled as
`R16_UNORM` with each channel taken from the texel or a constant as its selects say, as a one-byte
texture's are.

**Why:** Grand Theft Auto V's loading-screen lighting samples its point-light shadow cubes and a
64 x 64 x 32 `16_UNORM` array with `dim:2D` through one texture slot. Each was refused as not the
shape the module samples.
