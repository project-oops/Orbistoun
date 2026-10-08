# D774 - A packed float texture is sampled as it is

**Status:** decided
**Date:** 2026-10-08
**known_by:** published (`gfx10-rsrc.json`, and the Vulkan format definitions)

An image descriptor whose format is `GFX10_FORMAT_10_11_11_FLOAT` (36) binds its texels detiled and
untouched, one word a texel, as `TextureEncoding::Float11_11_10`. The host samples them natively as
`B10G11R11_UFLOAT_PACK32`, the packing D773 writes. The texels are staged one level as they are,
since averaging their bytes would mix the bit fields.

Such a texture is read only under the identity selects, or with alpha as the constant one. A
packed texel's channels cannot be moved within the word the host samples, so any other select is
refused rather than approximated. Its cache entry and hash are kept apart from the same bytes read
as `Rgba8`.

**Why:** the pass after PPSA02664's scene samples the HDR target D773 made drawable, as a
`10_11_11_FLOAT` texture. That draw was refused for an unbound texture, and nothing after it
retired.
