# D941 - A depth-only draw is sized by its depth surface

**Status:** decided
**Date:** 2026-10-21
**known_by:** published

A draw that writes no colour renders at its depth surface's extent. It writes no colour where no
colour buffer is bound (`CB_COLOR0_INFO` `COLOR_INVALID`, D779), where target 0's `CB_TARGET_MASK`
channels are clear, or where `CB_COLOR_CONTROL.MODE` is `CB_DISABLE` (bits 6:4 zero,
`gfx10.json`). Its extent is `DB_DEPTH_SIZE_XY` (`gfx10.json`, byte `0x2801C`: `X_MAX` in bits
13:0 and `Y_MAX` in 29:16, each the side less one, as `ac_descriptors.c:1049` writes them). A draw
that writes colour is sized to its colour buffer, as before (D702). A layered depth-only draw
(D936) is recognised the same way. Where the stream never wrote the depth size, the colour
registers' extent is kept.

**Why:** Grand Theft Auto V draws its point-light shadow faces, 256x256 and 512x512, with a 60x67
colour buffer still programmed from an earlier pass (`CB_COLOR0_INFO` `0x40710`) and its writes
off. Each face was drawn into a 60x67 attachment, so the faces of one cube disagreed in size and
the lighting that samples them was refused.
