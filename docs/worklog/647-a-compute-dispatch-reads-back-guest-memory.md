# 647. A compute dispatch reads back guest memory, where a guest's result lives

**2026-09-16** - the compute path binds the guest-memory window at binding 1 and reads it back, so a
guest dispatch's writeback is captured; it read only the observation window before, which is right for
a translated shader and wrong for a guest's - the gap worklog 635 named

## The gap, and why it was one

Worklog 635 named it precisely: `dispatch_into` bound a caller's buffer at **binding 0** - the
observation window a *translated* shader reports its registers through - and read that back, backing
binding 1 (guest memory) with a throwaway it discarded. That is exactly right for verifying a shader
this project translated, whose output is the register dump. It is wrong for a **guest's** compute
shader, whose result is in guest memory - binding 1 - which nothing read.

The graphics path had already learned this: a mesh draw binds the resident guest-memory window at
binding 1 and reads it back (worklog 641, 645). Compute now does the same, so the two agree on where
guest memory is and how it is observed.

## What was built

- `dispatch_into` is replaced by two entries that read **both** bindings. `dispatch_bound` binds two
  caller-owned buffers - a resident observation at binding 0, the resident guest-memory window at
  binding 1 - and returns both, destroying neither. `dispatch_reading_window` is the same with a
  throwaway observation created and destroyed here, for a dispatch that bound no observation buffer.
  Both reuse the existing `dispatch_core`, which already released everything but the buffers.
- The backend's `dispatch_compute` binds the window (the resident buffer uploaded once, worklog 645)
  at binding 1 and keeps its read-back as `last_window` - the guest's writeback - alongside the
  observation as `last_output`. The observation is a `BindBuffer`'s resident buffer when one is bound,
  a throwaway otherwise, so the two existing dispatch tests (which write and read binding 0) are
  unchanged.

## Made to fail

`a_dispatch_reads_back_the_guest_memory_window` (device): the window is seeded with known words and a
shader that ignores it is dispatched; the window comes back the seed - proof it was bound at binding 1
and read back, where the observation-only path returned zeros. This is the compute mirror of the
graphics window read-back, and made to fail against a dispatch that read only binding 0.

## What is interim, named as such

- **Verified by read-back, not by a guest shader writing it**, the same honest limit as the graphics
  window (worklog 641): no hand-assembled compute fixture writes binding 1, only a real translated
  guest shader does, so this pins that the window is bound and read - a guest shader storing to it is a
  captured frame away.
- The observation window (binding 0) is still read back for translated-shader verification; the two
  bindings stay separate, because a guest address must not reach the observation area (an out-of-range
  store would rewrite registers a test asserts on) - the reason `dispatch` gave them separate buffers
  all along.

## Gate state

`cargo test --workspace` **2418 passed, 0 failed** (+1 device-gated; the two existing dispatch tests
pass unchanged through the new path); `cargo clippy --workspace --all-targets -D warnings` clean; fmt
clean; identity scan exit 0. No commit.
