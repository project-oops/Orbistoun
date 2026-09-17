# 565. A console's two shaders draw together, and a silently dropped offset is why they could not

**2026-09-14** - orbistoun-translate, orbistoun-gen, orbistoun-shader and orbistoun-gpu-vulkan,
after worklog 561

The GL cube's vertex program and its pixel shader - both written by oops-sdk, both run on a
retail console, both captured in oracle record A - now **draw a frame together** on a GPU. The
vertex program fetches its vertices out of guest memory as a mesh shader, exports their
positions and colours, and the pixel shader interpolates the colour and writes it. Every pixel
comes back the colour the vertex carried.

Two things stood between here and there, and the second was a genuine silent wrongness.

## 1. The window has a base

Worklog 561 found that every memory access in a guest's shader is refused because the
guest-memory window is anchored at address zero and a guest's buffers are not. A window now has
a base: `Window { base }`, passed to `translate_windowed`, subtracted before the index is masked
and before the range is checked.

The check is the same shape it was and gains a property worth naming: unsigned arithmetic does
the work of two comparisons, because an address below the base wraps to something enormous and
fails the one test as surely as an address past the end does.

The base is thirty-two bits, which is what an address is by the time it reaches that code: a
flat access names its address in a register pair and the translation reads the low half. A guest
whose buffers straddle four gigabytes needs the high half too, and that is separate work.

## 2. The offset was being dropped, silently

With the window in place the vertex program drew - and its colours came out black. A pixel
shader reading black where green was stored is the shape of an address that is wrong by a
constant.

It was. A flat access carries a byte offset beside its address, and the solved operand layout
**had no field for it**: every probe in the corpus left the offset at zero, the reference prints
it only when it is not zero, so the solver had no evidence the field existed and correctly
declined to invent one. The translator then had nothing to add, and every access with an offset
read or wrote the word at offset zero instead.

The GL cube's vertex program fetches its position, colour and texture coordinates from one
address at offsets 0, 16 and 32. It was reading its position three times over.

Nothing failed. That is the point: this is the class of fault the project's whole
measure-don't-transcribe apparatus exists to prevent, and it survived because the probes were
uniform in one respect nobody had noticed.

## 3. Three fixes, each caught by a different instrument

- **Probes that vary the offset**, which is what the solver needed. It then reported four
  opcodes *unsolvable* rather than wrong: the offset had no candidate field, because twelve and
  thirteen bits were both missing from the widths it tries. The refusal is the design working.
- **The solver took the fewest operands any sample had.** One zero-offset probe therefore hid
  the field from every probe that carried it. It now takes the most, solving a position from the
  samples that have it - a position only one sample carries is ambiguous and still refused.
  That change immediately exposed a second thing: `done`, `compr` and `vm` were missing from the
  solver's modifier list and present in the decoder's, whose comment says the two agreeing is
  what makes a solved layout comparable with a printed one. True, and until now unenforced.
- **The width was wrong by one bit, and the differential test said so.** With the neighbouring
  cache hint never set in any probe, a thirteen-bit window explained every sample as well as the
  twelve-bit field does, and the solver wrote thirteen. A compiled instruction in the `minmax`
  fixture sets that hint, and the differential test against real compiler output reported an
  offset of 4096 where the reference printed none. Probes with `dlc` set pinned it to twelve.

That is the third time a field has solved one bit too wide because a neighbouring flag was never
exercised - `unorm` beside the image mask was the second. The lesson is written where the widths
are chosen: **a modifier is skipped as an operand and is still worth setting in a probe**.

## 4. What the frame is and is not

It is two shaders a console ran, translated here, drawing together, with the vertex data placed
where the console's shader looks for it.

It is **not** the console's frame. The vertices are this test's, because the oracle record
captured the command stream and the shader payload and not the vertex buffer. The record's pixel
hash remains a different claim.

The canary store still does not land, and that is now correct rather than a mystery: the window
covers the vertex buffer and the canary is two hundred kilobytes further on. A window is a span,
and a shader reaching outside it is refused - which the test asserts, so a window that silently
grew to cover it would fail here.

## 5. Files

- `crates/orbistoun-translate/src/model.rs` - `memory_base`, the base in the index and the
  range check, the offset added to a flat address, and a three-operand read that tolerates a
  fourth.
- `crates/orbistoun-translate/src/wavefront.rs`, `src/lib.rs` - `Window` and
  `translate_windowed`.
- `crates/orbistoun-gen/src/operands.rs` - the maximum operand count, the two widths, the
  modifier list aligned with the decoder's.
- `tools/shader-fixtures/probes/memory.s` - offsets, and the cache hints that pin their width.
- `crates/orbistoun-shader/data/opcode-operands.toml` and the recordings - regenerated.
- `crates/orbistoun-gpu-vulkan/tests/console_fragment.rs` - the frame, asserted.

## Next

1. The image subsystem, for record B's textured shader.
2. A window per buffer, or a window the pipeline derives from the submission: one span covers a
   vertex buffer or a canary, not both.
3. `REQ-20260914T1720Z-9c4a`, which settles the payload split D688 assumes.
