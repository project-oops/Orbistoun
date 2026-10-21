# D923 - Every guest image format maps to a host format or refuses by name

**Status:** decided
**Date:** 2026-10-20

An image descriptor's `FORMAT` is one code of Mesa's `GFX10_FORMAT` enum. `formats.rs` lists every
code with its line in `gfx10-rsrc.json`, and a test checks that list against Mesa's file. Each code
gets one of two answers:

- **sampled**: its layout, the Vulkan format that holds the same bits, where each of the guest's
  `X` to `W` sits in that format, and the format a CPU decode widens it to;
- **refused**: a reason, given by name.

A texture in such a format reaches the backend as `TextureEncoding::Guest`, which carries the code,
its selects, and the guest's bytes unchanged. The backend asks the device whether it can sample the
holding format. If it can, the bytes go up untouched. If it cannot, `formats::decode` widens them on
the CPU to a format every Vulkan implementation must sample. `BC1`-`BC7` decode the same way through
`bc.rs`.

A code's bit layout is read off its name, the way Mesa's generator reads it
(`gfx10_format_table.py:103-121`). The leading numbers give the channel widths from the most
significant end, so `X` is the lowest field. This is `published`. What the texture unit does around
that layout is `assumed` until obSCEne measures it (`REQ-20261020T1300Z-tx01`):

- what a select of a missing channel returns, which is refused until then;
- whether sRGB decoding happens before the selects;
- the meaning of the scaled and unpublished codes, which stay refused.

Tiling at every element size comes from `swizzle.rs`. It holds the exclusive-or equations that
`tools/addrlib/equation` reads off addrlib for seven swizzle modes at 1, 2, 4, 8 and 16 bytes per
element. A test checks them against the sixteen layouts `tiling.rs` models by hand.

A draw texture that no other reader binds goes to `guest_texture` (`pipeline.rs`), as long as it is
a flat, one-level, uncompressed, tiled 2D image in a float format. Its elements are detiled by the
mode's equation and bound as `Guest`.

**Why:** orbistoun sampled about eleven formats, and each new one meant a hand-written path chosen
for the title that needed it. A guest texture in any other format dropped its draw, and the reason
given did not name the format. With a table, every code is either read exactly or refused by name.
The CPU decode covers hosts that cannot sample a format, `BC` blocks among them.

**Rejected:**
- A decoder crate for the block formats. The algorithms are short, published (Khronos Data Format
  1.3 sections 21-23), and testable against the device's own decode, so writing them costs nothing
  in licences or provenance.
- Translating packed formats to `R8G8B8A8` always. Where the device samples a format natively, the
  guest's bytes go up unchanged, at a quarter of the bandwidth. The CPU decode is only the fallback.
