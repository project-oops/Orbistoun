# D873 - A DCC target's keys are read block by block

**Status:** decided
**Date:** 2026-10-14
**known_by:** published

A colour target whose DCC keys are not all one state is expanded one compress block at a time
before its draws. The key byte for each block is found by GFX10's key equation:
- a block cleared to `0000` is written as zeros;
- a block cleared to the register (`GFX8_DCC_CLEAR_REG`, `0x20`) is written as the colour
  `CB_COLOR0_CLEAR_WORD0/1` hold;
- an uncompressed block is kept.

Every key is then marked uncompressed. A compressed block, whose encoding is not published, is
refused with nothing written, as before.

The equation is Mesa's: `gfx10_nir_meta_addr_from_coord` (`ac_nir_surface.c` 11-57) over the
`gfx10_bits` addrlib's `Addr2ComputeDccInfo` gives for the console's configuration. It is read by
`tools/addrlib/dcc` for a single-sample `64KB_R_X` surface of four- and eight-byte texels, pipe
aligned or not, and carried as data in `dcc.rs`. The tool checks it against
`Addr2ComputeDccAddrFromCoord` for every compress block of three surfaces under three pipe XORs
(0 of 319140 differ at eight bytes, 0 of 160320 at four), and `dcc.rs` tests the port against six
of addrlib's addresses for each texel size.

**Why:** PPSA21564's third submission draws into a 16_16_16_16_FLOAT target its own fast clear
left as 81916 blocks cleared to the register and 4 cleared to `0000` (clear colour
`[0, 0xbc003c00]`). No block is compressed, so the surface is exact once each key is matched to
its block; without that, its draws stopped the queue.

**Not covered:** a mip chain's keys, a surface whose data has a pipe-bank XOR, and texel sizes the
equation was not read for, each refused by name.
