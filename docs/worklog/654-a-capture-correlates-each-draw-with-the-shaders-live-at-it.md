# 654. A capture correlates each draw with the shaders live at it

**2026-09-17** - `correlate_draws` joins a command stream's draws and dispatches to the shader
addresses live when each issued, servicing oops-libs' capture-glue request (inbox `-26aa`) - the piece
that turns a readable capture into an answerable one

## Why this, and why it is glue rather than new decoding

oops-libs asked (inbox `-26aa`) for the plumbing that turns a captured GPU dump into the analysis this
repository already does, in three thin pieces sitting on code already here. The one with real value is
the second: a stream on its own is a flat list of register writes and draws, and the question worth
asking - *which shaders did this draw run* - is the association between them, which nothing kept. Every
part needed was already built and measured: `register_writes`, `draw_calls`, `dispatch_calls`,
`shader_candidates`, and the most-recent-write-before-the-draw rule they all share. What was missing
was the join.

## What landed

`crates/orbistoun-gpu/src/registers.rs`:

- `DrawOrDispatch` - a draw (`DRAW_INDEX_AUTO`/`DRAW_INDEX_2`) or a dispatch (`DISPATCH_DIRECT`), with
  a `packet_offset()` that orders it in the stream.
- `DrawCorrelation { work, shaders }` - a unit of work paired with the shader candidates live when it
  issued.
- `correlate_draws(walk, body, vocabulary)` - reads the stream once into writes, draws and dispatches,
  orders the work by position, and for each reassembles the shader candidates from the writes that
  precede it. A capture read from a file walks straight into it (`walk(&bytes)` then `correlate_draws`).

Re-exported from `lib.rs` alongside `DrawCall`, `DrawKind`, `draw_calls`, which the correlation API
now exposes.

## What it deliberately does not do

- **No descriptor-table pointer.** `-26aa`'s piece 2 also names "which descriptor-table pointer was
  live". No register mapping for one is measured - the shader-address map itself is a hypothesis with
  no oracle (D091) - so attaching a guess would be the plausible-output this file refuses (D010). The
  correlation carries the measured half (shader addresses) and says so.
- **The candidates stay candidates.** The join inherits D091's caution rather than adding to it: these
  are reported, never dispatched on.
- **Piece 3 is not here.** Feeding a capture into the `orbistoun shaders` ranking verb is a CLI change
  and a separate unit; a fresh request should carry it if wanted (the inbox's own "need more → new
  request" rule).

## Tests, including the join's guard

Four, on synthetic streams built from the existing `stream`/`command` helpers:

- `each_draw_is_correlated_with_the_shader_live_when_it_issued` - bind A, draw; rebind B, draw; each
  draw carries *its own* bind, not the other's.
- `a_draw_before_any_shader_bind_correlates_to_no_shader` - **the guard for the join**: a bind that
  comes *after* a draw is not live at it, so the draw's shaders are empty. Without the before-this-point
  filter this draw would be falsely attributed to the later shader; made to fail by removing the filter.
- `a_dispatch_and_a_draw_are_correlated_in_stream_order` - both kinds correlate, ordered by position.
- `a_capture_read_from_a_file_walks_into_per_draw_shaders` - the acceptance end to end: a dword stream
  written to disk, read back, walked, and reported as a draw carrying the shader live at it.

## Gate state

`cargo test -p orbistoun-gpu --lib` **61 passed, 0 failed** (+4); clippy clean; fmt clean;
`orbistoun-gpu-vulkan` still builds on the new re-exports; identity scan exit 0. No commit.
