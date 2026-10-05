# D738 - A format load converts by the draw's descriptor

**Status:** decided
**Date:** 2026-10-05

A `buffer_load_format_*` converts what it fetches by the format its buffer descriptor names -
`FORMAT` and `DST_SEL_X..W` in the descriptor's fourth word - not by anything in the instruction.
radeonsi builds a vertex buffer's descriptor per draw and fetches 8- and 16-bit vertex elements
this way (`si_nir_lower_vs_inputs.c`), so the conversion a vertex shader performs is a fact about
the draw, as the geometry a primitive shader reads is (D730).

A shader with a format load is therefore translated per draw: the pipeline traces the buffer each
access reads through (D733), resolves each descriptor's fourth word from the draw's user data and
the tables it reads, and translates the module for those words, keyed by them, once per set
however many draws share it. The translator converts through the typed-buffer format table, whose
codes are `GFX10_FORMAT`'s, and applies the selects; a select of a channel the format lacks, a
format it does not convert, or a format load whose descriptor the draw did not resolve is refused
by name.

The alternative, deciding the format in the module at run time, would emit every format's
conversion for every lane of every load and choose among them; the per-draw module is exact and
costs one translation per vertex layout.
