# D817 - A depth compare samples the depth target at its address

**Status:** assumed
**Date:** 2026-10-12

A texture descriptor whose surface is depth-tiled (`SW_MODE` one of addrlib's `ADDR_SW_*_Z*`:
4, 8, 16, 20, 24) in a format a depth target has (`16_UNORM`, `32_FLOAT`,
`X24_8_32_FLOAT`), sampled by a
depth-compare instruction, is the depth target at the same address: the pipeline names it by
that address (`depth_target_id`) rather than reading its bytes, and the backend samples the
depth attachment it keeps under that id, through a sampler comparing by the descriptor's
`DEPTH_COMPARE_FUNC` (`SQ_IMG_SAMP_WORD0` bits 14:12, `gfx10-rsrc.json`), whose values 0-7 are
`VkCompareOp`'s in the same order (`radv_tex_compare`, `radv_sampler.c`).

**Why:** depth targets are never written to guest memory (the depth tiling is unmeasured,
`depth.rs`), so the bytes at the address are not the picture; the attachment is. radeonsi
samples a depth texture through the same surface address it rendered it at
(`si_make_texture_descriptor` over the one `si_texture`).

**Rejected:**
- Writing the attachment back in `64KB_Z_X` and reading it as a texture: a layout written
  and read only by orbistoun, whose correctness no oracle checks.
- Sampling the placeholder: a shadow everywhere or nowhere, a plausible wrong frame.

A sample that does not compare reads the same target's depth, uncompared, as `.r`: Vulkan ignores
`OpTypeImage`'s Depth operand, so a plain sample of the depth view returns the depth. A descriptor naming an address no depth target was drawn at is left unbound, and its draw
refused.
