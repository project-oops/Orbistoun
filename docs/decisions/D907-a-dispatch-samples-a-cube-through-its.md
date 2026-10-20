# D907 - A dispatch samples a cube through its six faces

**Status:** decided
**Date:** 2026-10-20

A compute program may sample a `TYPE` CUBE image as a cube (`image_sample* dim:CUBE`, with the face
coordinates and face id ACO gives it). That image's six faces are read as a six-layer array
(D897's placement) and bound as a Vulkan image created `CUBE_COMPATIBLE`, seen through a `CUBE`
view. An image whose view is not six layers is refused.

An 8-byte `64KB_R_X` array takes the 4-byte array's layer terms, which addrlib gives as the same
(`tools/addrlib/equation 64KB_R_X 8 32 array`: `0x800, 0x400, 0x200, 0x100`).

**Why:** Grand Theft Auto V's loading-screen program samples a 32 x 32 `16_16_16_16_FLOAT` cube,
and was refused as "not the shape its program addresses".
