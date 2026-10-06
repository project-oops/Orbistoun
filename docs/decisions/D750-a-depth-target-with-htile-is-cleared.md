# D750 - A depth target with HTILE is cleared where its HTILE says so

**Status:** decided
**Date:** 2026-10-06

A depth target whose `DB_Z_INFO` enables `TILE_SURFACE_ENABLE` carries its HTILE metadata's
address (`DB_HTILE_DATA_BASE`) and `DB_DEPTH_CLEAR`. A fill of that metadata - from the stream's
own `DMA_DATA`, or a compute dispatch's buffer left holding one word - that covers every 8x8 tile
with tiles in the cleared state (`ZMASK` zero) clears the backend's depth attachment to
`DB_DEPTH_CLEAR`, as a fill of the depth surface itself already does. A dispatch buffer counts as a
fill whether or not its words changed: nothing here updates HTILE as depth is written, so a frame's
clear rewrites the words the last left, and is still a clear.

**Why:** radeonsi fast-clears a depth buffer with HTILE by writing only the metadata
(`si_clear.c`, `HTILE_Z_CLEAR_REG` / `HTILE_ZS_CLEAR_REG` in `ac_descriptors.h`), through a compute
buffer clear first (`si_buffer.c`); the depth block then reads every cleared tile as
`DB_DEPTH_CLEAR`. Nothing here read HTILE, so the depth attachment kept the last frame's depth and
every later frame's geometry failed its `Less` test: CRFT00001 drew its sky and no terrain, its
HTILE at `0x401870000` cleared each frame by a dispatch. A tile takes 32 bits and the metadata's
blocks only pad that (`gfx10addrlib.cpp`), so a fill short of every tile's word is not a clear of
the whole surface.

**Rejected:**
- Decoding HTILE per tile: a partial clear is not something a title has been seen to do, and a
  per-tile depth model is a second depth implementation.
- Writing HTILE back as the depth block would after a draw: it would let the clear be detected by
  change, at the cost of writing hardware metadata the guest never asked to be written.
