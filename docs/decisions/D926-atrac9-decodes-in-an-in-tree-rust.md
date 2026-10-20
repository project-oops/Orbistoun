# D926 - ATRAC9 decodes in an in-tree Rust reimplementation of LibAtrac9

**Status:** decided
**Date:** 2026-10-20

`orbistoun-atrac9` decodes ATRAC9: a pure crate with no guest knowledge and no dependencies,
written in Rust from LibAtrac9 (Alex Barney, MIT). Each function cites the file and lines of
LibAtrac9's C source it follows. The format's tables are transcribed from that source and
attributed to it under its licence. That covers the Huffman codes (`codes.rs`, generated from
`huffCodes.c` by reading its arrays in order), the band and unit layouts, the quantiser steps,
the scale-factor weights and the band-extension scales. The arithmetic matches LibAtrac9's step
for step, in double precision, unfused and in the same order, so a frame decodes to the same
samples.

Two refusals depart from the reference. A unit whose precision passes 30 bits is refused
(`PrecisionRange`), where the reference indexes past its fine step table. A frame whose bits run
past its data is also refused (`Truncated`), where the reference reads whatever memory follows.

The tests carry no encoded audio from anywhere else. Each test encodes its own frames, writing
the fields in the order the reader reads them. The checks go against definitions computed in the
test, not against the decoder's own code:

- the transform against the DCT-IV;
- a two-tone signal, transformed with the window the synthesis window inverts, against the same
  signal after decoding;
- Huffman-coded spectra against their textbook synthesis;
- intensity stereo and band extension against the format's own description of them.

**Why:** ATRAC9 is the codec the platform's audio engine decodes for most first-party titles. The
engine's codec 1 is "ATRAC9 Decoder" in the console's own log (`20261016-000300-eboot.log` 1045).
No crate decodes it, and the brief for this work names LibAtrac9 as the reference.

**Rejected:**
- Linking LibAtrac9's C through FFI: a C toolchain in every build, and `unsafe` at a boundary
  that parses hostile bytes. CLAUDE.md keeps parsing free of `unsafe`.
- Decoding through ffmpeg's ATRAC9 decoder: LGPL. A library that size would also be linked for
  one codec.
- Writing the decoder from a format description alone: none is published. The Huffman tables
  have to be the encoder's to the bit, and LibAtrac9 is the only open statement of them.
