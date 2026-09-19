# 678. `sceVideoOutRegisterBuffers` keeps the addresses a frame lives at

**2026-09-17** — inbox `-6a86`. `video_out_register_buffers` bound only the handle and the count and
threw the buffer addresses away; `video_out_submit_flip` bound only the handle and the flip argument
and did not record which buffer it presented. Three things were blocked on that one discard - a
renderer needs an address the guest agreed to write into, `Reach::Presented` needs a flipped buffer to
read back, and framebuffer diffing needs bytes to diff - and none could start. The addresses are kept
now.

## What changed

`port::Port` gained two fields: `buffers: Vec<u64>` (the registered guest addresses, in index order)
and `last_flip: Option<u64>` (the index the guest last flipped).

`video_out_register_buffers` now binds `args[1]` (the start index) and `args[2]` (the address array),
reads `count` guest addresses out of that array under the identity mapping, and stores them into the
port's set at the given index. A **null** address array is the one case refused - it registers buffers
a renderer could never read, so it earns the video-out error rather than storing a row of zeros - which
is also the guard the read needs, since reading `count` quadwords from address zero would fault. The
count is bounded by a defensive ceiling (`MAX_REGISTERED_BUFFERS = 16`), stated as a bound on a garbage
count rather than a hardware maximum, so an arbitrary count cannot walk the read off into unmapped
memory.

`video_out_submit_flip` records `args[1]` as `last_flip`; its doc no longer says `buffer_index` is
"accepted and not modelled". `flip_mode`/`flip_arg` stay unmodelled for the reasons they already gave.

## Test

`registering_buffers_stores_their_addresses_and_a_flip_records_the_index`: registers three buffers at a
known address triple and asserts they come back in index order; flips buffer 2 and asserts `last_flip`
is `Some(2)`; then registers a null array and asserts it is refused with the video-out error and stored
nothing - the guard watched failing.

## What it unblocks, and what it is not

This is the storage `-9b1f` (the `Reach::Presented` rung) reads from and the address a renderer would
present. It is **not** a renderer or a presenter: nothing scans these buffers out, and a flip still
completes on submit (there is no vblank). It records where the frames are, which is the prerequisite
every downstream piece named and none had.

## Gate state

`cargo clippy -p orbistoun-video --all-targets -- -D warnings` clean; `orbistoun-video` 8 tests pass;
fmt clean; `./bin/orbistoun prose` exit 0; identity scan clean. No commit.
