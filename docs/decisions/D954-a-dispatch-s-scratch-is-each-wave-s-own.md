# D954 - A dispatch's scratch is each wave's own private memory

**Status:** decided
**Date:** 2026-10-24
**known_by:** published

A compute dispatch whose program asks for scratch (`COMPUTE_PGM_RSRC2.SCRATCH_EN`) is carried out,
not refused. Each wave holds `COMPUTE_TMPRING_SIZE.WAVESIZE` bytes, counted in 1 KiB units before
GFX11 (Mesa `src/amd/common/ac_gpu_info.c:1617`, applied at `src/amd/common/ac_shader_util.c:1078-1079`).
A dispatch that asks for scratch without the stream having written that register, or with a size of
zero, is refused, and so is one asking for more than 256 KiB a wave. The size is compiled into the
module, so it is part of the dispatch module's key.

The wave's offset into the scratch ring arrives in the scalar register after the user data and the
workgroup ids (`src/amd/vulkan/radv_shader_args.c:648-663`: the workgroup ids, the group size, then
`scratch_offset`). The group size (`TG_SIZE_EN`) is still refused, so the offset register is the one
after the last enabled workgroup id.

No other wave and no later dispatch reads a wave's scratch, so where the ring lies in guest memory is
never observed. The module holds scratch as a private array instead. The array is indexed by the
access's byte address from the wave's scratch base, over four. The offset register reads zero, as
every register the module seeds nothing into does, and the descriptor's base is not read. Where one
invocation is the whole wave, the array is the wave's scratch word for word, laid out as the hardware
lays it out. Where each invocation is one lane (D760, D880), the array holds that lane's own words: a
word whose low bits name another lane is not reachable from this one.

A program reaches scratch two ways, and both are translated:

- **Through a buffer descriptor**, adding the wave's offset as the access's scalar offset. This is
  ACO's MUBUF scratch (`src/amd/compiler/aco_ir.cpp:1945-1958` builds the descriptor: the lane id
  added at an index stride of the wave's width, raw bounds; radv sets its swizzle,
  `src/amd/vulkan/radv_queue.c:270-274`; the access is
  `src/amd/compiler/instruction_selection/aco_select_nir_intrinsics.cpp:3412-3419`). A buffer load or
  store whose `SOFFSET` field (its second word's bits 31:24, `aco_assembler.cpp:587`) names the offset
  register is a scratch access. Its byte is AMD's published RDNA 2 buffer address less the
  descriptor's base: the scalar offset, plus, with `index` the address register's index and the lane
  id, `offset` its offset and the immediate, and `IS` the index stride,
  `((index / IS) * stride + (offset / 4) * 4) * IS + (index % IS) * 4 + offset % 4` where the
  descriptor swizzles, the element four bytes from GFX9 on (`aco_ir.cpp:1953-1954`), and
  `offset + stride * index` where it does not. The descriptor's fields are read when the program
  runs. One that does not add the lane id is not a scratch descriptor, and its access is an escape.
- **Through the flat encoding's scratch segment**, bits 15:14 one (`aco_assembler.cpp:906-907`).
  Its address is the lane's own offset into its private space: `SADDR` off is the signed twelve-bit
  immediate alone, `null` the address register plus the immediate, a scalar register that register
  plus the immediate (`aco_assembler.cpp:903-904`, `:931-936`). The module lays it out as the buffer
  form's swizzle with an index stride of the wave's width. Only a lane's own words are reachable
  through this form, so its layout is not otherwise observed.

Dword accesses one to four wide are translated both ways, and the byte and half loads of the
scratch segment and `buffer_load_ushort`. A dword must be aligned, and a byte or half must lie within
its dword. An access the array does not hold reads zero, stores nothing, and is an escape, so the
dispatch's result is refused after it as D746 refuses any escape.

A scratch access reaches no buffer and no window. The buffer tracer, the window placement and the
dispatch's write spans leave it out, by the same test the translation makes. A program that writes
the offset register is refused, since its scratch accesses would add something the module reads as
zero.

**Why:** Grand Theft Auto V's in-world frame runs two full-screen compute passes with scratch, the
programs at `0x74005b854a00` (240 by 135 groups) and `0x74005b857200` (32400 groups), and both were
refused outright, so what they draw never landed.

**Rejected:**
- Binding the scratch ring as a guest buffer. Each wave's offset is the hardware's to choose, so a
  translated wave cannot know which part of the ring is its own, and a private array is what the
  memory is.
- Recognising a scratch buffer access by its descriptor alone. Its words are known only when the
  program runs, and the tracer and the window must decide before it does. The scalar offset is in the
  encoding.
- Mapping a flat-segment access that lands in the private aperture onto scratch. `SH_MEM_BASES`,
  which places that aperture, is not modelled (D878), and neither form of scratch the compilers
  emit uses it.
