# D837 - A sampler's table border colour is read from the guest's table

**Status:** decided
**Date:** 2026-10-14

A sampler descriptor whose `BORDER_COLOR_TYPE` is `REGISTER` (3) takes its border colour from the
border colour table, not from one of the three built-in colours. The table is wherever
`TA_BC_BASE_ADDR` (and `_HI`) last put it in the submission, in 256-byte units. The colour is entry
`BORDER_COLOR_PTR` (`SQ_IMG_SAMP_WORD3` bits 11:0): sixteen bytes, four floats, red to alpha.

The decoder names the entry (`BorderColour::Table`). Once a submission's textures are bound, the
pipeline reads each named entry from guest memory and binds its four floats
(`BorderColour::Custom`). The Vulkan backend samples that colour through
`VK_EXT_custom_border_color`, as a colour of the sampler's own with no format. The device enables
the extension where it offers both `customBorderColors` and `customBorderColorWithoutFormat`.

Two cases are refused, as before:

- an entry with no table to read, or one outside guest memory;
- a device without the extension.

**Why:** a GL guest's `GL_TEXTURE_BORDER_COLOR` is any colour. oops-gl writes it into the table
(`OOPS_GL_BORDER_TABLE_OFFSET`) and names entry 0, as radeonsi does (`si_translate_border_color`).
Refusing the texture refused the draw, and nothing after it retired. In gl1-probe, that stopped
every rendering check from `border-and-mirror` on.

**Rejected:**
- Substituting the nearest built-in colour: a sample the guest did not ask for.
- Reading the table at decode time: the decoder has no memory, and a table set once per frame
  belongs to the submission, not to the descriptor.
