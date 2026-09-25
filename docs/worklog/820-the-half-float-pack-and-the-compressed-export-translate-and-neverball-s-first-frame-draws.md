# 820. The half-float pack and the compressed export translate, and Neverball's first frame reaches the backend whole — 454 commands carried out, none refused, a 1920x1080 frame

**2026-09-24** — worklog 819 left Neverball's textured fragment shader translating to byte `0x488` of
`0x500`, where its export slot begins: two `v_cvt_pkrtz_f16_f32` and a compressed export
(`exp mrt0, v4, v5 done compr vm`). The SDK moved to the half-float export on 2026-09-23 because an
8_8_8_8 target on this part with RB+ takes that format.

## The pack

- **Named by the reference**: `v_cvt_pkrtz_f16_f32 v4, v4, v5` and `v5, v6, v7`, the export slot's own
  words, join `tools/shader-fixtures/texture.s`; regenerated in the toolchain VM, only the table and the
  fixture pair pulled back. VOP2 opcode 47, `v_cvt_pkrtz_f16_f32_e32`, the one addition.
- **Translated** by `pack_halves`: per lane, each source narrowed to a half and the first placed low,
  the second high — the order ACO relies on when it packs `(r, g)` and `(b, a)` for an FP16 colour
  export (`aco_select_ps_epilog.cpp:170-178`). The narrowing is a new `Model::float_bits_to_half`, the
  inverse of the existing `half_to_float_bits`, through `OpFConvert` so overflow, denormals and NaNs are
  the device's.
- **The rounding is an assumption, written down**: the instruction rounds toward zero, `OpFConvert`
  rounds as the device chooses (nearest-even on every driver this has run on). They differ by at most
  one unit in a half's last place — below an eight-bit channel's resolution, but not claimed exact.
- Test `two_floats_pack_into_one_register_as_halves` (on the device): 1.0 and 2.0 pack to
  `0x4000_3c00` — exact values, so any rounding gives that word, and a swap or a conversion does not.

## The compressed export — and a hole it closes

`export`'s own doc said the compressed bit and the write mask "are not among the operands the decoder
solved … nothing here may claim to honour them" — and then nothing refused them either. A compressed
export was translated as four whole floats: two packed bit patterns as red and green, and whatever two
other registers held as blue and alpha. Plausible output, the thing principle 3 forbids.

Both now come from the instruction's first word, cited from ACO's assembler (`aco_assembler.cpp:1001`,
`:1005`): **`COMPR`** (bit 10) unpacks `(r, g)` from the first source and `(b, a)` from the second,
low half first, through `half_to_float_bits`. **`EN`** (bits 0-3): all four channels store the
`vec4`; **none stores nothing** — an export raised only to end the wave; a partial mask is refused by
name, because storing a whole `vec4` would overwrite channels the guest meant to keep.

**The gate found the mask's other half.** Six hand-assembled test shaders across
`orbistoun-gpu-vulkan`'s `translated_export`, `translated_interpolation`, `translated_sampling` and
`storage_image` exported with the word `0xF800_0000` while their own comments said
`exp mrt0 v0, v1, v2, v3` — `EN` zero, *no* channel enabled — and asserted the exported colour on the
attachment. They passed only because the translator ignored the mask. On the hardware an export that
enables nothing writes nothing, so the words were the slip: all six are now `0xF800_000F`, what their
comments say. (`orbistoun-shader`'s `export_decode` keeps its word; it tests decoding, not output.)

## What Neverball does now

```
2 of 2 shader candidates translated to a module a backend can bind
orbistoun: a submission reached the NVIDIA GeForce RTX 5070 Ti backend:
  454 command(s) driven, 0 refused, frame 1920x1080, in 10618 ms
```

Its first frame's draw submission — 450 draws, the title screen — is carried out by the Vulkan backend
end to end, where one tick ago every draw was refused. The cube is unchanged (its shaders translate, 16
commands, none refused, clear self-test passing).

The frame the backend produces is still dropped at `render.rs`, which is inbox request
`REQ-...1f07`, now fully exercisable: writing it out is what makes it comparable with the hardware's
image of the same title screen. And the guest still waits on the fence of that submission, because
it executes after the run rather than at submit (D705/D710); both are the next units.

## Gate state

`tools/shader-fixtures/texture.s`, `crates/orbistoun-shader/data/mnemonics.toml` and
`tests/fixtures/texture.{gcn,txt}` (generated); `crates/orbistoun-translate/src/model.rs`
(`pack_halves`, `float_bits_to_half`, the export's `COMPR`/`EN` handling and their constants) and
`tests/execute.rs` (the test); the six export words in `crates/orbistoun-gpu-vulkan/tests/`.
`orbistoun-translate` 125 execution tests and the rest pass; every `orbistoun-gpu-vulkan` test binary
passes on the device.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
