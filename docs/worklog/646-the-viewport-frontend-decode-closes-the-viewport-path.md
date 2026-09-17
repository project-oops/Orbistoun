# 646. The viewport frontend decode: a stream's scissor reaches the backend as a SetViewport

**2026-09-16** - the frontend decodes the generic scissor a stream sets and emits it as a
`SetViewport`, which the backend already applies (worklog 644); the register offsets and layout came
from mining a hardware draw capture, unblocking what 644 had left deferred

## What unblocked it

Worklog 644 landed the backend half of the viewport - a `SetViewport(Rect)` restricts a draw to a
rectangle - but deferred the frontend decode, because no capture in hand set a scissor and several
candidate registers (`PA_SC_GENERIC_SCISSOR`, `_VPORT_SCISSOR`, the window and screen scissors) had
nothing to disambiguate them.

obSCEne's end-to-end draw oracle (`166-agc/primitive-draw`, sweep `20260916-223136`, request
`-a1f7`) resolved both. Mining its dumped DCB byte stream - every packet a `SET_CONTEXT_REG` from base
`0xA000` - the draw sets **all four** scissors to its 64x64 frame, and the application-controlled one
is the **generic scissor**: `PA_SC_GENERIC_SCISSOR_TL` at `0xA090` = `0x80000000`, `_BR` at `0xA091`
= `0x00400040`. The capture also carries the viewport *transform* (`PA_CL_VPORT_XSCALE`… at `0xA10F`,
32/32/32/32/0.5/0.5 for a 64x64 viewport), but that is the NDC-to-pixel map, a separate concept the
command does not carry; the scissor is what "restrict rasterisation to a rectangle" means.

## What was built

- `decode_scissor(top_left, bottom_right) -> Scissor { x, y, width, height }` and
  `scissor_at(writes)`, reading `GENERIC_SCISSOR`'s two corners. The bit layout is not from memory:
  it is `PA_SC_WINDOW_SCISSOR_TL`/`_BR` (oops-mesa `src/amd/registers/gfx10.json`), x in bits 14:0 and
  y in 30:16, with a `WINDOW_OFFSET_DISABLE` flag at bit 31 of the top-left that is masked off - and
  the register offsets `0xA090`/`0xA091` are the same file's, cross-checked against the DCB.
- `submit` emits a `RenderCommand::SetViewport` with the decoded rectangle when a stream set the
  scissor. A stream that set none emits none, so a draw covers the whole target - which is every
  capture but this one, so no existing submission changed.

This closes the viewport path: the frontend decodes and emits, the backend restricts the draw
(worklog 644), and the two meet on a rectangle.

## Made to fail

- `a_scissor_decodes_its_rectangle` (decode): the measured `0x80000000`/`0x00400040` is 64x64; a
  sub-rect with a non-zero origin (`0x0008_0010`/`0x0038_0030` = (16,8)-(48,56)) exercises the
  top-left the full-frame case cannot, and an inverted rectangle is empty rather than an underflow.
  Made to fail against a decode that read the `WINDOW_OFFSET_DISABLE` bit into `y`.
- `the_scissor_reads_the_live_registers` (accessor): the latest write to each corner wins, and a
  stream missing a corner has no scissor to guess.
- `a_scissor_write_reaches_the_backend_as_set_viewport` (frontend): a `GENERIC_SCISSOR` write pair
  through `submit` produces a `SetViewport` carrying the decoded sub-rect.

## What is interim, named as such

- **The measured value is the full frame.** The oracle draws to its whole 64x64 target, so the one
  hardware scissor value in hand is `(0,0)-(64,64)`; the sub-rect cases are constructed to the mined
  layout, the same way the T# and CB-register decodes are tested against constructed values.
- The generic scissor is the one decoded; the screen, window and per-viewport scissors the same draw
  also sets are not, because they are the same rectangle here and the generic one is the application's.

## Gate state

`cargo test --workspace` **2417 passed, 0 failed** (+3: two decode, one frontend); `cargo clippy
--workspace --all-targets -D warnings` clean (`submit`'s draw/dispatch emission split into
`push_geometry_commands` to stay under the line limit); fmt clean; identity scan exit 0. `Scissor`,
`decode_scissor` and `scissor_at` re-exported from the crate root. No commit.
