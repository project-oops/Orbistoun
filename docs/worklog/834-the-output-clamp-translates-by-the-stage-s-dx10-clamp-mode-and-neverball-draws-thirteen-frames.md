# 834. The output clamp translates by the stage's DX10_CLAMP mode, and Neverball draws thirteen frames

**2026-09-24**: worklog 833 left Neverball's seventh submission unexecuted. Its new pixel shader
failed at instruction `0x3c0`, which "clamps its result to [0, 1]". The clamp is the long-form
vector ALU's output modifier, bit 15 of the first word. It is how a compiler folds GLSL's
`clamp(x, 0.0, 1.0)` into the instruction that produces `x`.

## What the clamp needs to know

On a 32-bit float result, the clamp holds the value to `[0, 1]`. What it does to a **NaN** is not a
property of the instruction but of the shader: the `DX10_CLAMP` mode bit. Set, a clamped NaN becomes
zero; clear, it passes through.

- The bit is bit 21 of the stage's `SPI_SHADER_PGM_RSRC1`, per Mesa `S_00B848_DX10_CLAMP` and
  `gfx103.json`:
  - `RSRC1_GS` is at `0x2C8A`;
  - `RSRC1_PS` is at `0x2C0A`.
- The open-toolchain GL context sets it differently per stage (oops-sdk `gl_draw.c` / `gl_context.c`):
  - `RSRC1_GS` `0x622c0042` has it set;
  - `RSRC1_PS` `0x000c0010` has it clear.

A translation that assumed either value would be wrong for the other stage.

## The change

- **The pipeline:** `user_data_layouts` (the per-stage entry state already decoded from `RSRC2`) now
  also reads each stage's `RSRC1`. It carries `UserData.dx10_clamp: Option<bool>`, which is `None`
  when the stream set no `RSRC1`.
- **The translator:**
  - The wavefront model holds the mode, and `Model::dx10_clamp` answers it. The default is `None`,
    which is what every model given no `RSRC1` has.
  - `Modifiers::read_allowing_clamp` reports the flag instead of refusing it. Every other caller keeps
    `Modifiers::read`, which still refuses it.
- **Where the clamp is applied:** `long_form_arithmetic` applies it, as `clamp_unit`, to a result
  `result_is_f32` names as a 32-bit float:
  - an `_f32` operation that is not a comparison;
  - or a conversion *to* `f32`. `v_cvt_i32_f32` ends in `_f32` and produces an integer.
- **How it is computed:** `FMin(FMax(x, 0), 1)`, with the NaN taken by its own select. GLSL.std.450
  leaves `FMin`/`FMax` undefined on a NaN.
- **Where it is still refused by name:**
  - a clamp in a stage whose mode is unknown (compute, and every direct `translate`);
  - on a select or a scaled multiply-add;
  - on an integer result, where the bit means saturation.

## Tests

- `the_output_clamp_holds_a_float_result_to_the_unit_range` (device): 3 → 1, -1 → 0, 0.5 untouched.
  A NaN becomes zero with `DX10_CLAMP` and stays a NaN without it. The two NaN assertions differ by
  mode, so the test fails if the mode is ignored.
- `a_clamp_on_a_result_it_does_not_clamp_is_refused`: a clamped `v_cndmask_b32_e64` is refused with
  the mode known.
- The existing `a_modifier_that_is_not_translated_is_refused` still passes. A clamp with no mode is
  refused, and the message now says why.

## What moved

**Neverball draws 13 frames in a 150 s run: 14 of 14 submissions to completion, 0 refused.** Each
frame takes 10-12 s.

The latest frame shows the title scene's backdrop and, new, the level's geometry, in cyan and black.
But much of it **fans into one point near the screen centre** (960, 545), which is where a vertex at
NDC (0, 0) lands. Vertices reading zeros is the likely cause: vertex data outside the shader's memory
window (D711 places it from the constant base, at a fixed size), where every read is bounds-checked to
zero. That is the next unit.

The cube is unchanged (`frames-confirmed: 0x5`).

## Gate state

`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
