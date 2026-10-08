# D775 - An anisotropic sampler samples at the host's anisotropy, at the ratio radv encodes

**Status:** decided
**Date:** 2026-10-08
**known_by:** assumed (radv's sampler encoding, `radv_sampler.c:64-116`, read in reverse)

A sampler descriptor whose `XY_MAG_FILTER` or `XY_MIN_FILTER` is anisotropic (`ANISO_POINT` 2,
`ANISO_BILINEAR` 3, `gfx6.json` `SQ_TEX_XY_FILTER`) is sampled with the host's anisotropic filter.
The base filter is point or bilinear, as the value names it. The maximum anisotropy is
`1 << MAX_ANISO_RATIO`, from `SQ_IMG_SAMP_WORD0` bits 11:9.

radv writes exactly these fields from a `VkSamplerCreateInfo`:

- the XY filter is the anisotropic form of the Vulkan filter when `maxAnisotropy > 1`;
- `MAX_ANISO_RATIO` is `min(log2(maxAnisotropy), 4)`.

So the host sampler built here is the Vulkan sampler from which radv would build this descriptor.
`ANISO_THRESHOLD`, `ANISO_BIAS` and `ANISO_OVERRIDE` tune the hardware's own footprint; they are
not read. A ratio of zero under an anisotropic filter is one sample, as radv never encodes and
the host does not distinguish. A host device without `samplerAnisotropy` samples with the base
filter alone.

**Why:** PPSA02664 samples its 12-level `BC3` texture through a 16x anisotropic sampler. The
decoder refused every anisotropic filter as not reproduced, so the draw ran with no texture bound,
was refused, and nothing after it in the submission retired. No host reproduces a GPU's
anisotropic footprint exactly, so refusing it would block every title that asks for it. The
choice is assumed until a framebuffer comparison against hardware measures it.
