# 675. The `Submission` carries the pipeline state its decodes produced

**2026-09-17** — inbox `-73c3`. Eight register decodes had landed with no consumer: `colour_target_at`,
`depth_control_at`, `stencil_control_at`, `blend_control_at`, `colour_swizzle_mode_at`,
`target_mask_at`, `detile_colour_target`, `detile_texture` were referenced only by their own modules,
their own tests and the `pub use` that exports them. Three of them landed this session (669/670/671)
and the carry side did not move. So a draw issued through the backend would have run with no depth
test, no blend, into an attachment of the right size at the wrong address. `submit` now carries them.

## What `Submission` gained

`pipeline::Submission` grew five fields, each `Option`, each populated in `submit` from the accessor
that already decoded it:
- `colour_target: Option<ColourTarget>` - colour target zero's **base** and extent
  (`colour_target_at`, `CB_COLOR0_BASE`, worklog 655).
- `colour_target_tiling: Option<SwizzleMode>` (`colour_swizzle_mode_at`, worklog 657).
- `depth_control: Option<DepthControl>` (worklog 669).
- `stencil_control: Option<StencilControl>` (worklog 670).
- `blend_control: Option<BlendControl>` (worklog 671).

Each is `None` when the stream set its register nowhere - the state is read, never assumed, so a draw
with no `DB_DEPTH_CONTROL` carries no depth test rather than a default one.

## The base is decoded now, and the stale comment said otherwise

`submit`'s colour-target comment still read "the target's guest address is not [decoded], because the
register that carries it is disputed (D702)". That predates worklog 655, which decoded `CB_COLOR0_BASE`
(cited Mesa, `-a1f7`-tested) - the detile already uses it. Corrected: the `targets` map stays
extent-keyed for D702's reason (the size register is corroborated by both sources), but the base is
decoded and now rides on `colour_target` so a backend can find the attachment in guest memory.

## Test

`tests/pipeline.rs::a_stream_sets_the_pipeline_state_the_submission_carries` drives one stream that
sets all six registers (`CB_COLOR0_BASE`/`ATTRIB2`/`ATTRIB3`, `DB_DEPTH_CONTROL`, `DB_STENCIL_CONTROL`,
`CB_BLEND0_CONTROL`) with the values `registers.rs`'s own tests pin, and asserts the submission carries
each decoded value - base `0x2000e0000`, `Tiled64KbRX`, depth `LessEqual`, stencil `Keep`/`ReplaceTest`,
blend enabled `SrcAlpha`/`OneMinusSrcAlpha`. `grep depth_control_at` now shows a caller in
`pipeline.rs`, outside `registers.rs`, its tests and the re-export.

## What is done and what waits

The acceptance - a `Submission` carrying the state, asserted, with the accessors consumed - is met. The
request also wanted `RenderCommand` to gain the variants a backend configures a pipeline from
(depth-stencil, blend); those wait with the backend itself (`-36c0`, deferred), because a
`RenderCommand` variant with no backend to act on it is a seam that pays no rent yet. The state is now
on the `Submission` where those commands would read it from when the backend is built.

## Gate state

`cargo clippy -p orbistoun-gpu --all-targets -- -D warnings` clean; gpu lib 81, pipeline tests 23
passed; fmt clean; `./bin/orbistoun prose` exit 0; identity scan clean. No commit.
