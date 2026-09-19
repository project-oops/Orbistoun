# 695. Video-out: the two undeclared calls named, the attribute kept, the flipped buffer readable

**2026-09-19** — inbox `-420c`: three small gaps in `crates/orbistoun-video/src/lib.rs`, all on the
road to a `presented` rung (`-9b1f`). Two title calls reached the loader as bare hashes; the
`attribute` argument the guest hands `sceVideoOutRegisterBuffers` was read past and dropped; and the
buffer addresses `-6a86` kept were sealed inside the private `mod port` with no reader outside the
crate — so nothing above `orbistoun-video` could find the frame the guest just flipped, or its layout.

## What changed

- **The two calls are declared.** `sceVideoOutSetBufferAttribute2` and `sceVideoOutGetOutputStatus`
  now sit in the `guest_module!` at arity 6 — the trampoline's full capture, the shape `agc.rs` uses
  until a call is measured — declared and **not** implemented (D500). PPSA02664 imports both and makes
  each twice from two title modules; declared, the loud-stub policy answers them by name rather than
  letting them arrive as bare hashes. `orbistoun-cli symbols` now lists both under `libSceVideoOut`
  (`0x5f71b004b8b9343e` / `0xffa362dc55ebd3ba`, argc=6), derived live from the ABI names — no
  `symbols/` regeneration needed, and `symbols-audit` stays green. Declared-not-implemented, so no
  knowledge entry is owed (`every_implemented_function_is_written_down` confirms it).
- **The attribute pointer is kept.** `video_out_register_buffers` now binds `args[4]` and stores it on
  a new `pub attribute: u64` on `Port`. It points at the buffers' pixel format, extent and tiling —
  which a reader of a flipped frame needs to read the bytes back and know their layout.
- **The flipped frame is readable from outside.** A crate-level `pub fn last_flipped_buffer() ->
  Option<(u64, u64)>` returns, for the port that last flipped, `(guest address of the presented
  buffer, attribute pointer)`. It reads a process-global "last flipped port" handle that
  `video_out_submit_flip` records via `port::note_flipped`. This is what `-9b1f`'s `presented` arm was
  missing: an address *and* the layout to interpret it, reachable without peering into `mod port`.

## Two guards made to fail before they were trusted

The new test `the_attribute_is_kept_and_the_flipped_buffer_reads_back` registers a set with a non-null
attribute, flips index 1, and reads the address+attribute back through the public function. Writing it
surfaced two races against the shared, process-global port state — both caught by *watching the test
fail*, not by counting a pass (principle 3):

- **The global last-flipped handle is shared across tests.** `last_flipped_buffer` reads a
  process-global handle any test's flip sets, so a parallel flipping test could overwrite it between
  this test's flip and its read. Fixed with a `serial()` lock the five flipping tests now hold — the
  new one and the four that predate it.
- **Bus 7 was already held open.** The first draft opened `open_on(7)`, but the already-open-refusal
  test holds bus 7 open for the whole run (ports never close), so in the parallel suite that test grabbed
  bus 7 first and this open was *refused* — `register_buffers` then got a bad handle and returned
  `0x8028fe0b` instead of `0`. The failure was invisible in isolation and only appeared in the full
  suite. Moved to bus 5 (buses 1/3/4/6/7 are taken). Three back-to-back full-suite runs: 9 passed, 0
  failed, no race.

## Gate state

`orbistoun-video` clippy `-D warnings` clean; `cargo fmt -p orbistoun-video --check` clean; workspace
tests pass (`cargo test --workspace` exit 0); `orbistoun prose` exit 0; `status --check` exit 0 after
`status --write` reconciled the generated blocks — **Functions declared** 965 → 967 (these two
declarations) and **Recorded behaviours** 814 → 815, measured 81 → 82 (worklog 694's `sceAgcDcbSetFlip`
entry, whose regeneration had not been persisted in the working tree); `knowledge-audit` 27 pass;
`symbols-audit` exit 0; identity scan clean. No commit.

**One honest caveat on `./bin/orbistoun check`.** Its `cargo fmt --all -- --check` step fails — but not
on anything here. orbistoun path-depends on `selfish-elf`/`selfish-nid`/`selfish-container`, so
`cargo fmt --all` follows into the sibling `selfish` workspace and checks all of it, including
`selfish-title/src/param.rs`, which is committed in a rustfmt-non-clean state and which orbistoun does
not even depend on. It is pre-existing (fails identically without this change) and belongs to selfish;
flagged for a separate selfish session rather than absorbed here.
