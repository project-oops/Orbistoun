# 637. The colour target's dimensions are decoded from CB_COLOR0_ATTRIB2

**2026-09-16** - the render target the executor draws to is a fixed 64x64 square (worklog 636);
this decodes the guest's real dimensions from the register that carries them, the measured half of
replacing that square with the guest's target

## What this adds

The first graphics frame (worklog 636) drew into a fixed 64x64 attachment, named as interim: "A
guest's target has its own dimensions and clear, decoded from the CB registers a stream sets; that
decode is the next graphics piece." This is that decode.

`decode_colour_target_extent(attrib2)` reads a `CB_COLOR0_ATTRIB2` value into a `ColourTargetExtent`
- width minus one from bits 27:14, height minus one from bits 13:0, each fourteen bits, each one
below the pixel count. `colour_target_extent_at(writes)` pulls it from the live value of that
register among a submission's register writes, the most recent write winning, and answers `None`
when the stream never set it - a target size this would otherwise have to guess is one it refuses to
guess (D010).

This is a pure decode, the shape `buffer_descriptor_at` and `dispatch_calls` already have: a measured
primitive that lands before the executor consumes it, so it can be checked against the oracle on its
own before anything depends on it.

## The oracle, and why the decode is self-checking

`CB_COLOR0_ATTRIB2` is register `0xA3B0` - the `SET_CONTEXT_REG` base `0xA000` plus offset `0x3B0`.
The GL cube capture (`tests/captures/agc-gl-cube-fw1240-a`) writes `0x01dfc437` there, and that
value is not transcribed from a reference: it is in the command stream a retail console drew on
firmware 12.40 and hashed to `0x9dbfe189` over all 1920 x 1080 pixels (worklog 539). The decode
turns `0x01dfc437` into exactly `1920 x 1080` - the frame the record states - and no other pair comes
out of that value. The register naming rests on oops-sdk's own `gl_draw.c`, but the decode does not
need to trust it: the dimensions it produces are the dimensions the console hashed.

## Made to fail

- `a_colour_target_decodes_its_width_and_height` - `0x01dfc437` decodes to 1920 x 1080; a decode that
  dropped the minus-one would give 1919 x 1079, one that swapped the fields 1080 x 1920. Zero decodes
  to a one-by-one target (the value is one below the count), and a maximal width leaves the height at
  its minimum, so the fields do not bleed into each other.
- `the_extent_reads_the_live_register_and_is_absent_when_unset` - a stale extent overwritten before
  the draw is not the one read; a stream that wrote only an unrelated register offers no extent
  rather than a fabricated one.

## What is interim, named as such

- **Only the dimensions are decoded, not the clear colour.** Worklog 636 named "dimensions and clear"
  as the next piece; the dimensions have a measured register and the clear does not. Neither GL cube
  capture writes a clear-colour register - a solid clear on this hardware is a fast-clear through
  CMASK state or a drawn quad, and no oracle value for it is in hand. Inventing a register offset for
  it from memory is exactly the mistake the identity guard's sibling rule warns about, so the clear
  colour stays the interim black the executor already uses, and the honest gap is recorded rather
  than papered over (principle 3).
- **Nothing consumes the extent yet.** The executor still sizes its attachment from `RENDER_WIDTH`.
  Carrying the decoded extent to the backend is the next unit: a colour target becomes a resource the
  frontend makes resident (a new `Resource::RenderTarget { width, height }` arm, the way the enum was
  built to grow), `SetRenderTargets` selects it, and `draw_graphics` sizes its attachment from the
  resident target instead of the constant. That threads the residency seam (D701) rather than putting
  render state on the submission envelope, which is why it is its own unit and not a field bolted on
  here.

## Gate state

`cargo test -p orbistoun-gpu` green (2 new lib tests); `cargo clippy -p orbistoun-gpu --all-targets`
clean; fmt clean. `ColourTargetExtent`, `decode_colour_target_extent` and `colour_target_extent_at`
re-exported from the crate root beside the other register decodes. Full-workspace gates running.
