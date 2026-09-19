# 697. The doc gate was red behind an earlier one, on stale intra-doc links

**2026-09-19** — while verifying worklog 696 against `./bin/orbistoun check`, the run reached the
`cargo doc --workspace --no-deps` step (with `-D warnings -D rustdoc::broken_intra_doc_links`) and
failed on eleven intra-doc-link errors across three graphics crates. None was from 696; all were in the
committed tree. They had been latent because recent ticks failed an earlier gate - a doc drift, a fmt
drift, a status-number drift - and the doc step, which runs last, was never reached. The same shape as
selfish's worklog 61: a gate reports green because it was never actually run to the end.

## What was broken

All were public-item documentation pointing at something rustdoc would not resolve in a public page:

- **Public docs linking private helpers** (`private_intra_doc_links`, implied by `-D warnings`):
  `detile_image`/`detile_colour_target`/`detile_texture` linked `[element_bytes]` and
  `[detile_surface]` in `orbistoun-gpu/src/tiling.rs`; `implementations` linked
  `[agc_no_op_returns_ok]` in `agc.rs`. The helpers are private, so the link never renders in the
  public page anyway.
- **Prose parsed as a link.** `agc.rs` described a packet layout as `body[0]` / `body[3]` / `body[4]`
  without backticks, so rustdoc read `[0]`/`[3]`/`[4]` as shortcut links and could not resolve them.
- **A stale reference.** `orbistoun-gpu-vulkan/src/compute.rs` linked `[dispatch_into]`, a function
  removed when the resident-buffer split landed (worklog 635/647 renamed it); `dispatch`'s doc still
  named it as the resident-binding variant, which is now `dispatch_bound`.

## The fix

Each was turned into a code span rather than a link (`` `element_bytes` ``, `` `detile_surface` ``,
`` `agc_no_op_returns_ok` ``, `` `body[0]` `` and so on) - the item names read the same and a public
page cannot link a private helper regardless. The stale `dispatch_into` was corrected to the current
`dispatch_bound` (a code span, because `dispatch_bound` is `pub(crate)` and a link from the public
`dispatch` would trip the private-link lint that started this). No `#[allow]` was added: the lint is
right in every case, so the docs were changed to satisfy it rather than silenced.

Behaviour is untouched - these are documentation comments only.

## Gate state

`cargo doc --workspace --no-deps` under the CI rustdoc flags now exits 0, and `./bin/orbistoun check`
passes end-to-end (all checks passed) - the first fully green run this session. `orbistoun-gpu` and
`orbistoun-gpu-vulkan` clippy clean; fmt clean; identity scan clean. No commit.
