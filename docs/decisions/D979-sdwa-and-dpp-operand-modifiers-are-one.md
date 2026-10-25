# D979 - SDWA and DPP operand modifiers are one shared path over any eligible instruction

**Status:** decided
**Date:** 2026-10-25
**known_by:** published

An SDWA or DPP modifier word changes how an instruction reads its sources and writes its result.
It does not change what the instruction computes. So the modifiers are applied around the plain
instruction in one place, for every instruction that has the form. Before, each instruction was
listed with its own translation of the form.

**SDWA** (`model/sdwa.rs`), on any one- or two-source vector instruction whose operands are at most
32 bits wide:

- Each source is read whole, then reduced to the byte, word or dword its select names, zero- or
  sign-extended. A float or bit-pattern source then has its negate and absolute value applied to its
  top bit: bit 15 for a half, bit 31 otherwise. An integer source with either is refused.
- The selected sources are staged where the plain short form reads them. The first goes in the
  register past the 256 an instruction can name. The second goes in the destination, whose old bits
  are read first. The plain form then runs.
- A float result is scaled by the output multiplier and then clamped, a half in single precision
  and narrowed again. Clamping or scaling an integer or bit-pattern result is refused.
- The result is placed into the destination select with its fill: zero, the part's sign above it
  with zero below, or the old bits. A sixteen-bit result fills a whole-dword destination
  zero-extended.

A compare keeps its own path, since it writes a lane mask rather than a register. A part of a float
source is now read as any source's part is. An accumulating form (`fmac`), one taking a literal, one
writing a scalar, or one moving registers by index has no SDWA form, and is refused by what it is.

**DPP** is one shape for DPP16 and DPP8, on any one- or two-source vector instruction:

- DPP8 (first-source codes 233 and 234, `fi:1`) reads the lane of its group of eight that its three-bit
  select names. Its source lane is always valid, and one the execution mask leaves out reads zero
  unless `fi:1`. That is DPP16's rule with every row and bank enabled and `bound_ctrl` set.
- The DPP16 source negate and absolute value (23:20) apply as SDWA's do. The first source's apply after
  the lane permute; the second source's apply as it is staged in the destination.
- The first source is found by its marker in the operand list, so a carry's sources are where its
  layout puts them. A carry is refused only where a lane may be left unwritten, since its carry
  bit cannot be given back. A select reading the condition mask is translated.

**Why:** Grand Theft Auto V's three ray-tracing programs were refused on `v_min_u32_sdwa` and
`v_mul_u32_u24_sdwa`. Every such refusal so far had been answered by adding one more name to a
list. The offline oracle had also called the wavefront emitter directly, which bypassed the
crate's refusal of untranslated modifier words. There the model's fall-through treated a
two-source SDWA form as its one-source plain form and dropped the second source's select. The
oracle now translates through the crate's entry, as the pipeline does.

**Sources:** AMD's published RDNA 2 instruction set (SDWA, DPP16, DPP8); LLVM's
`AMDGPUModifierSyntax` for each field's meaning (`UNUSED_SEXT` "sign-extend upper bits, zero
lower bits"; DPP8 `fi:0` "fetch zero when accessing data from inactive lanes"); `llvm-mc` for
`gfx1030` for the encodings and for which instructions have each form.

**Rejected:**
- Extending the per-instruction lists. Each new title would bring another refusal of an instruction
  whose form means the same thing.
- More staging registers past 256. The destination holds the second source correctly, because no
  instruction with an SDWA form reads its destination.
