# D899 - An array slice takes its pipe-bank XOR, and DCC on a slice is expanded

**Status:** decided
**Date:** 2026-10-19

Slice `s` of a 2D array in a 64 KiB `_X` swizzle mode lies a whole chain after the slice before it
(D859). Its pipe-bank XOR is the surface's own XORed with `s`'s low four bits reversed into the four
pipe bits. Higher bits add nothing (`gfx10addrlib.cpp:2230`). `tools/addrlib/equation <mode>
<bytes> 16 array` reads the same terms (`0x800`, `0x400`, `0x200`, `0x100` on in-block offsets) for
`64KB_R_X` at one to eight bytes, `64KB_D_X` at four and sixteen bytes, and `64KB_Z_X` at two and
four. A colour target drawing slice `SLICE_START` is placed with that XOR.

A pipe-bank XOR in any 64 KiB `_X` layout moves each 256-byte piece of a block to the piece the
XOR names. Every such layout is modelled under any XOR: the pieces are put back, the texels read or
written as the layout lies with no XOR, and the pieces are moved again.

A DCC target drawing slice `s` is expanded as a single slice is (D873), on that slice alone:
- **Keys:** `s` slices of keys after the first, one level's key bytes each. Each key is found by
  the key equation with `s` as its slice coordinate. A pipe-aligned equation has slice terms
  (Mesa's `gfx10_nir_meta_addr_from_coord`, `ac_nir_surface.c:11-57`).
- **Blocks:** a chain after the first slice's, under the slice's XOR.

`tools/addrlib/dcc <bytes> <aligned> 6` checks Mesa's formula, with the slice's keys at
`dccRamSliceSize * s`, against `Addr2ComputeDccAddrFromCoord` for every block of six-slice
surfaces.

**Why:** Grand Theft Auto V draws into slice 1 of an eight-byte `64KB_R_X` array with DCC, and was
refused. Its slice also lay at the wrong in-block offsets without the slice's XOR, which a single
slice never showed.

**Rejected:**
- Refusing a target whose combined XOR the layout's own offset function did not take: the XOR is
  the same piece move at every texel size, read off addrlib, so refusing it would only keep the
  wall.
