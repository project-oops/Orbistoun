# D773 - A packed float colour target is drawn in its own format

**Status:** decided
**Date:** 2026-10-08
**known_by:** published (Mesa's colour-format mapping and the Vulkan format definitions)

A colour target whose `CB_COLOR0_INFO` says `COLOR_10_11_11`, `NUMBER_FLOAT`, in the standard
component order is drawn into a `B10G11R11_UFLOAT_PACK32` attachment, through a view of the same
format. It is seeded from, and written back to, guest memory four bytes a texel, untouched. Mesa
targets `R11G11B10_FLOAT` at `COLOR_10_11_11`, and Vulkan's packed format holds the same bits:
red in bits 0-10, green in 11-21, blue in 22-31.

A target's encoding is carried with its resource (`TargetEncoding`: eight-bit `UNORM`, eight-bit
sRGB, or this one) in place of the earlier sRGB flag. It decides the attachment's image format,
the format it is drawn through, and the pipelines built for it.

**Why:** PPSA02664 renders its scene into a 1920x1080 `10_11_11` float target, the usual HDR
buffer. The executor drew only eight-bit targets, so the draw was refused as "not one a frame can
be written back to exactly", and the second submission stopped there.

**Rejected:**
- Drawing it as `R8G8B8A8` and converting on writeback: blending would clamp at one, and a value
  above one is the point of the format.
