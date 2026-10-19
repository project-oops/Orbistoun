# D897 - A dispatch fetches any layer of a 2D array

**Status:** decided
**Date:** 2026-10-19

A compute program's `2D_ARRAY` fetch or store reaches every layer the descriptor views, from the
first to the last that `SQ_IMG_RSRC_WORD4.DEPTH` names. The descriptor may be typed `2D`, `2D_ARRAY`
or `CUBE`; a cube's six faces are six layers. The module declares the image arrayed, with the layer
as the third coordinate. A layer past the last is out of bounds, as on the hardware: a fetch there
reads zero and a store there writes nothing. The layer count comes from asking the bound image for
its size. The host binds a 2D array view of that many layers.

On the console, each layer of a tiled array lies a slice of blocks after the one before
(`gfx10addrlib.cpp:4825-4848`). Its in-block offsets are XORed with the layer's low four bits
reversed into the pipe bits (0x800, 0x400, 0x200, 0x100, `gfx10addrlib.cpp:2230`). These are the
same terms a 32-bpp `64KB_R_X` volume's slices take (D823), so an array is read and written back as
such a volume is. The terms were read with `tools/addrlib/equation <mode> <bytes> 32 array` for
16- and 32-bit `64KB_Z_X` and 32-bit `64KB_R_X`, and only those are bound, at one level, with no
pipe-bank XOR of their own. A view from a later layer, and a linear array, are refused by name.

This extends D895, whose one-layer image is now an array of one layer.

**Why:** Grand Theft Auto V's compute copy reads one face of a six-layer 16-bit `64KB_Z_X` cube
through a `2D_ARRAY` fetch, the layer from its user data, and stores it to the same face of another.
This was the first wall its graphics queue met. Device tests fetch and store each layer of a
three-layer array, and nothing past the last.

**Rejected:**
- Binding only the layer the user data names: the host would have to know which user-data word a
  program takes its layer from, which holds for this copy and not for an array a program indexes
  per thread.
