# 669. `DB_DEPTH_CONTROL` decodes the depth- and stencil-test state

**2026-09-17** — inbox `-6e78` names the depth/stencil state registers among the pipeline state to
decode. `DB_DEPTH_CONTROL` is the first and most fundamental: whether the depth and stencil tests
run, whether depth is written, and the comparison each uses. It is now decoded into a typed struct,
the same pattern the scissor, colour-target and target-mask decodes already follow ahead of a
renderer.

## What it decodes

`DB_DEPTH_CONTROL` is context register `0xA200` (`SET_CONTEXT_REG` base `0xA000` + offset `0x200`),
cited from `oops-mesa src/amd/registers/gfx103.json` (byte `165888`, `165888 / 4` = `0xA200`) with its
fields at `STENCIL_ENABLE`[0], `Z_ENABLE`[1], `Z_WRITE_ENABLE`[2], `DEPTH_BOUNDS_ENABLE`[3],
`ZFUNC`[4:6], `BACKFACE_ENABLE`[7], `STENCILFUNC`[8:10], `STENCILFUNC_BF`[20:22]. The three compare
fields select from `CompareFrag` (`FRAG_NEVER` 0 .. `FRAG_ALWAYS` 7, cited from the same file), decoded
to a `CompareFunc` enum. `decode_depth_control` produces a `DepthControl`, and `depth_control_at`
reads the live register out of a submission's writes (most-recent-write-wins, `None` when unset -
the test state is not a thing to assume).

The two colour-write-on-depth-{fail,pass} interaction bits (30, 31) are a rarer feature and left
undecoded; the struct covers the test state a draw turns on, which is what a host configures a
depth-stencil pipeline from.

## Why it fits the pattern, not a premature seam

There is no renderer consuming this yet - as with the scissor and colour-target decodes, the decode
*is* the deliverable. Decoding a measured register into a typed struct with a test is orbistoun-gpu's
core activity, cited to Mesa and pinned by tests, not a speculative abstraction. A renderer's
depth-stencil pipeline derives directly from these fields.

## Tests

`the_depth_control_decodes_its_test_state` drives a value with a distinct comparison in each of the
three fields (`ZFUNC` LEQUAL, `STENCILFUNC` ALWAYS, `STENCILFUNC_BF` NOTEQUAL) plus the enable bits,
and asserts each field lands at its cited bit position - including a bit deliberately left unset
(`DEPTH_BOUNDS_ENABLE`), so a decode that misplaced a field would fail. It also checks the all-zero
value decodes to every test off and every compare `NEVER`, decoded rather than assumed.
`the_depth_control_reads_the_live_register_and_is_absent_when_unset` pins most-recent-write-wins and
the absent case.

## What remains on 6e78

The colour-target half was fully decoded (base/extent/tiling, worklogs 655/637/657); this adds the
first depth/stencil register. `DB_STENCIL_CONTROL` (the stencil op state) and the blend state
(`CB_BLEND0_CONTROL`) are the next same-shaped units, and `GE_CNTL`/`GE_PC_ALLOC` remain out of scope
(no host consumer - the mesh-shader path configures the geometry engine its own way).

## Gate state

`cargo clippy -p orbistoun-gpu --all-targets -- -D warnings` clean; gpu lib 77 passed; fmt clean;
`./bin/orbistoun prose` exit 0; identity scan clean. No commit.
