# 824. A live submission's window is placed at the base its vertex shader forms, and the cube draws its first pixels — a Gouraud-shaded face, 114,282 of them

**2026-09-24** — worklog 823 found that a live submission's shaders have no window onto their own
vertices: the submit path never placed one, and `Window.base` could not have held the cube's buffer
address above four gigabytes anyway. D711 records the mechanism taken; this is its implementation.

## What was built

- **`Window` gains its high half** (`orbistoun-translate` `wavefront.rs`): `base` stays the low thirty-two
  bits a translated shader compares against; the new private `high` is where the words are read from.
  `Window::spanning_address(address, words)` builds one from a full address and refuses a window that
  would cross four gigabytes; `Window::address()` gives it back. `read_guest_window` reads from the full
  address and the translation cache's salt carries it.
- **`constant_address_base`** (`orbistoun-gpu` `pipeline.rs`): the 64-bit base a program forms from two
  scalar registers written exactly once, by constant `s_mov_b32`, and fed to a carry-chained
  `v_add_co_u32` / `v_add_co_ci_u32` pair. Names are resolved the way the translator resolves them
  (`EncodingTable::mnemonic_for`), because the plain mnemonic table has no long-form (VOP3) names and the
  add the base feeds is a VOP3 — the first version asked the wrong table and found nothing.
- **`Pipeline::placing_window_from_shaders`**: each submission's window is placed, before anything is
  translated, at the vertex-stage candidate's base, spanning the largest power-of-two span (at most 2^16
  words) that is wholly readable and does not cross four gigabytes; no base leaves the window as it was.
  The live submit path builds its pipeline this way; every hand-placed caller is unchanged.
- **Test** `the_constant_base_a_vertex_program_forms_is_found_and_a_reassigned_one_is_not`, on the cube's
  console-run vertex program (oracle record B's payload): the base is `0x2_0090_0000` — the vertex loads'
  pair, the address worklog 561 recorded, the low half of which is what the capture tests anchor their
  hand-placed window at — and not the canary store's `0x2_0092_0000` behind it. With a second write to
  `s2` put in front, the vertex pair names nothing and the scan takes the next constant pair, never
  either value of the reassigned register.

## What the cube draws

```
cube: 16 commands, 0 refused, 1920x1080 - non-black pixels: 114,282 in 18,531 colours,
      bounding box (944, 0) - (1396, 858)
```

**The first pixels either fully-owned baseline has put in a frame.** One Gouraud-shaded face — blue
through magenta to near-white, the vertex colours interpolated across it — where the cube draws twelve
triangles. Each of the twelve draws carries its own vertex offset in user data (`s8`, set by
`SPI_SHADER_USER_DATA_GS_0` before every draw); that the frame shows one face suggests every draw is
reading the same offset, which is the next thing to check.

## What Neverball draws

Still nothing: its 454 commands run, both shaders translate, and not one pixel is lit. Its draws go
through the GL context's other vertex programs (the three- and four-parameter ones,
`vs-param3.s`/`vs-param4.s`), and whether the base is found for them and the window placed is the other
next question.

## Gate state

`crates/orbistoun-translate/src/wavefront.rs` (`Window`'s high half, `spanning_address`, `address`),
`crates/orbistoun-gpu/src/pipeline.rs` (`constant_address_base`, `scalar_destination_span`,
`place_window`, `placing_window_from_shaders`, the salt, the read, the test),
`crates/orbistoun-gpu/src/agc_driver.rs` (the live pipeline places its window), `docs/decisions/D711`.
`orbistoun-gpu` 96 library tests and the rest pass. `./bin/orbistoun check` green, decision and worklog
indexes regenerated, identity scan clean. No commit.
