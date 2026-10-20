# D908 - A dispatch reads and stores packed 11-11-10 floats

**Status:** decided
**Date:** 2026-10-20

A dispatch image of `10_11_11_FLOAT` (format 36) is three packed unsigned floats in a word: red in
bits 10:0, green in 21:11 and blue in 31:22, Vulkan's `B10G11R11_UFLOAT_PACK32`, as draws already
read it (D774). Its selects may be X, Y, Z and one, or W read as one: the fourth channel reads one
in the host's three-channel view as well. Any other select is refused, as for the other packed
formats.

**Why:** Grand Theft Auto V's loading-screen program stores into a 1024 x 1024 `10_11_11_FLOAT`
cube's faces with selects X, Y, Z, 1.
