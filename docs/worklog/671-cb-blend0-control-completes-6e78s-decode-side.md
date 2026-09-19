# 671. `CB_BLEND0_CONTROL` decodes the blend state, completing 6e78's decode side

**2026-09-17** — the last of 6e78's named pipeline-state registers with a host consumer.
`CB_BLEND0_CONTROL` carries colour target zero's blend state - a source and destination factor and a
combine function for colour, the same three for alpha, and the enable flags. It is now a typed struct
decoded from the measured register, alongside the colour-target, depth and stencil decodes.

## What it decodes

`CB_BLEND0_CONTROL` is context register `0xA1E0` (`SET_CONTEXT_REG` base `0xA000` + offset `0x1E0`),
cited from `oops-mesa src/amd/registers/gfx103.json` (byte `165760`, `165760 / 4` = `0xA1E0`). Its
fields decode into `BlendControl`:
- `COLOR_SRCBLEND`[0:4], `COLOR_DESTBLEND`[8:12], `ALPHA_SRCBLEND`[16:20], `ALPHA_DESTBLEND`[24:28] -
  each a `BlendFactor` (`BlendOp` enum, `BLEND_ZERO` 0 .. `BLEND_ONE_MINUS_CONSTANT_ALPHA` 20, cited).
- `COLOR_COMB_FCN`[5:7], `ALPHA_COMB_FCN`[21:23] - each a `CombineFunc` (`CombFunc` enum,
  `COMB_DST_PLUS_SRC` 0 .. `COMB_DST_MINUS_SRC` 4, cited).
- `SEPARATE_ALPHA_BLEND`[29], `ENABLE`[30], `DISABLE_ROP3`[31] - bools.

Both enums have reserved encodings (the `BlendOp` field is five bits with `21..32` reserved, the
`CombFunc` field three bits with `5..8` reserved). Those are carried as `BlendFactor::Other(raw)` /
`CombineFunc::Other(raw)` rather than mapped to a defined value - the same honest-unknown pattern
`SwizzleMode::Other` already uses, so a garbage or newer encoding is visible rather than silently
wrong.

## Tests

`the_blend_control_decodes_its_factors_and_functions` puts a distinct factor/function in each of the
six enum fields (so a misplaced field fails) plus the three flags, and checks a reserved value in each
enum decodes to `Other`. `the_blend_control_reads_the_live_register_and_is_absent_when_unset` pins
most-recent-write-wins and the absent case.

## 6e78's decode side is complete

Every named state register with a host consumer is now decoded: `DRAW_INDEX_2` (628),
`CB_COLOR0_BASE`/`ATTRIB2`/`ATTRIB3` (655/637/657), `CB_TARGET_MASK` (656), `DB_DEPTH_CONTROL` (669),
`DB_STENCIL_CONTROL` (670), and `CB_BLEND0_CONTROL` here. The depth/stencil/blend structs map directly
onto Vulkan pipeline state (`VkPipelineDepthStencilStateCreateInfo`, `VkPipelineColorBlendAttachment`)
so a renderer consumes them; the only named registers left, `GE_CNTL`/`GE_PC_ALLOC`, stay out of scope
because they have **no** such consumer - orbistoun's backend drives the geometry engine through its
own mesh-shader path rather than replaying the guest's GE config (principle 6). So 6e78 is complete
on the decode side but for two registers whose decode would be code no path exercises.

## Gate state

`cargo clippy -p orbistoun-gpu --all-targets -- -D warnings` clean; gpu lib 81 passed; fmt clean;
`./bin/orbistoun prose` exit 0; identity scan clean. No commit.
