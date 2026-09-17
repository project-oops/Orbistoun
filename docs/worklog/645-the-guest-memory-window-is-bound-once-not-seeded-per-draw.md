# 645. The guest-memory window is uploaded once and bound directly, not seeded per draw

**2026-09-16** - the direct-bind D703 deferred: a mesh draw binds the guest-memory window as a
resident buffer uploaded once, instead of copying its bytes into a fresh buffer every draw - the
graphics counterpart of compute's `dispatch_into`

## What this changes

Worklog 641 fed the guest-memory window by seeding it: each mesh draw created a binding-1 buffer and
copied the window's bytes into it, drew, read it back, and destroyed it. Correct, but a frame of many
draws re-copied the same window each time. D703 named binding a **resident** buffer directly - "upload
once" - as the better end state, deferred because it needs the framebuffer to bind a buffer it does
not own.

Now the backend uploads the window to a resident buffer once, keyed by the window's content, and every
mesh draw binds that buffer directly. A window that does not change across draws or frames is uploaded
once; when its bytes change, the stale buffer is destroyed and a new one uploaded, so it never grows
without bound.

## The ownership surgery, made safe

The delicate part D703 flagged: the framebuffer's draw creates and destroys everything it binds, and a
resident buffer must be bound but **not** destroyed. Mirroring compute's `dispatch_core` (which binds
caller-owned buffers and releases everything except them):

- `Bound` gained an optional `guest_buffer`; `build_pipeline` uses it for binding 1 when present and
  records `owns_guest_memory = false`, otherwise creates and owns one as before.
- `seed_memory` runs only for a window the pipeline created - a resident one is already uploaded.
- `release` destroys binding 1 only when `owns_guest_memory`, so a caller's buffer is left intact.
- The default path (`guest_buffer: None`) is byte-identical to before, so every existing caller and
  test is unaffected; only the new `draw_mesh_into` takes the direct-bind path.

The backend holds the window buffer and re-uploads it only when the content hash changes, rather than
routing it through the resident-resource id space: the window is frame state (D703), not a
guest-numbered resource, and a content-keyed field destroys its stale buffer where an id-keyed cache
would leak the old one until drop.

## Made to fail

- `the_window_is_uploaded_once_and_survives_the_draw` (device + mesh): two draws over one window both
  read it back correctly - which proves the first draw's release did **not** free the resident buffer,
  since a freed one would fault the second draw - and the upload count stays one, proving it was reused
  rather than re-uploaded. The two claims are inseparable: a byte-correct second read over a surviving
  buffer, uploaded once.
- `a_changed_window_is_re_uploaded_and_an_unchanged_one_is_not` (device + mesh): setting the same bytes
  again does not re-upload (count stays one); setting different bytes does (count two) and the new
  window reads back. Made to fail against a cache that never refreshed or one that refreshed every draw.

## Gate state

`cargo test --workspace` **2414 passed, 0 failed** (+2, both device+mesh; the existing mesh and window
tests pass unchanged through the new path); `cargo clippy --workspace --all-targets -D warnings` clean
(the buffer-upload unsafe is now one shared helper, and `render_over`'s read-back split into
`read_outputs`); fmt clean; identity scan exit 0. No commit.
