# 670. `DB_STENCIL_CONTROL` completes the depth-stencil test/op state

**2026-09-17** — the companion to worklog 669: where `DB_DEPTH_CONTROL` decoded the depth- and
stencil-*test* state, `DB_STENCIL_CONTROL` decodes the stencil *operations* - what happens to a
stencil value on each test outcome. Together they are the depth-stencil pipeline state a draw runs
under, and both are now typed structs decoded from their measured registers.

## What it decodes

`DB_STENCIL_CONTROL` is context register `0xA10B` (`SET_CONTEXT_REG` base `0xA000` + offset `0x10B`),
cited from `oops-mesa src/amd/registers/gfx103.json` (byte `164908`, `164908 / 4` = `0xA10B`). It
holds six four-bit fields, all of the `StencilOp` enum: front-face `STENCILFAIL`/`STENCILZPASS`/
`STENCILZFAIL` and their back-face twins `*_BF`. `StencilOp` is the full sixteen-value enum
(`STENCIL_KEEP` 0 .. `STENCIL_XNOR` 15, cited from the same file), decoded by `decode_stencil_op`.
`decode_stencil_control` produces a `StencilControl`; `stencil_control_at` reads the live register
out of a submission's writes (most-recent-write-wins, `None` when unset). The back-face set applies
only when `DB_DEPTH_CONTROL`'s `BACKFACE_ENABLE` is on, which the struct doc points at.

## Tests

`the_stencil_control_decodes_its_six_operations` puts a distinct op in each of the six fields
(`Keep`/`ReplaceTest`/`Invert` front, `Zero`/`AddClamp`/`Xnor` back) so a misplaced field fails, and
`the_stencil_control_reads_the_live_register_and_is_absent_when_unset` pins most-recent-write-wins,
that the untouched fields stay `Keep`, and the absent case.

## 6e78's decode side is nearly complete

Of 6e78's named state registers the colour target (base/extent/tiling) and the depth-stencil pair
(test state 669, ops 670) are now decoded. `CB_BLEND0_CONTROL` (the blend factors and combine
functions - a larger `BlendOp`/`CombFunc` decode) is the remaining state register; `GE_CNTL`/
`GE_PC_ALLOC` stay out of scope, having no host consumer.

## Gate state

`cargo clippy -p orbistoun-gpu --all-targets -- -D warnings` clean; gpu lib 79 passed; fmt clean;
`./bin/orbistoun prose` exit 0; identity scan clean. No commit.
