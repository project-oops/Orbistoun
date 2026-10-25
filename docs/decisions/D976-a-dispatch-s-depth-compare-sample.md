# D976 - A dispatch's depth-compare sample compares in the module

**Status:** decided
**Date:** 2026-10-25
**known_by:** guest-observed

A compute dispatch's `image_sample_c_lz` reads the image a fetch would read, bound whole as the
dispatch's other images are (D904), and needs no window. It makes the comparison in the module.
The module samples the texels as they are and compares each against the reference by the
sampler's function, filtered where the sampler magnifies linearly. This is how a pixel shader
compares an image that is not a depth target (D816).

A dispatch binds its images as their texels, and a depth target's as the host's depths (D894). No
host compare sampler reads either. The command processor reads each compared slot's sampler from
the dispatch's user data, which gives the function and the filter. A module translated before they
were read is translated again for them, as for its images' numeric types (D893). A compared slot
whose sampler cannot be read is refused by name.

`image_sample_c`, at the level a pixel's derivatives choose, stays outside the exemption: a
dispatch has no derivatives.

**The mode register.** `s_setreg_b32` and `s_setreg_imm32_b32` have their operand layouts from the
same probe round (`probes/hwreg.s`). A write of the flat scratch base is carried out (D964). A write
of the mode register, the wave's rounding and denormals, is refused by name. A module compiles its
float mode in from its dispatch's (D965), and a mid-program change would need every float
operation after it translated under the new mode. No program met so far writes it. Supporting it
waits for one that does, which then shows whether its values are known when the module is
translated.

**Why:** Grand Theft Auto V's ray-traversal program at `0x74005b858800` compares against a depth
image with `image_sample_c_lz` at 0x24f8. In a dispatch that writes guest memory, only accesses
checked against the window were admitted, and the compare sample was not among the bound-image
accesses. The program was refused as unchecked against its window.

**Rejected:**
- Binding a depth-format image and a compare sampler per dispatch: the dispatch's images are
  texels the host copied out (D894), not depth attachments, and a second binding path would serve
  one instruction.
- Leaving the compare to a sampler with comparison off: Vulkan leaves a depth-reference sample
  through such a sampler undefined.
