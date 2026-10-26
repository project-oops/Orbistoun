# D995 - A compare through a sampler the host cannot name reads it as the module runs

**Status:** decided
**Date:** 2026-10-26
**known_by:** guest-observed

A dispatch's depth-compare sample compares in the module (D976), by the sampler's function and
filter. The host finds the sampler in three ways, in order:

1. A table record or a user-data word names it, as before.
2. The trace names its four words: constants, including those a scalar instruction folds from
   constants, user data, or loads at fixed places. This is the trace a selected image's sampler
   takes (D983), and the walk is shared. The slot's first sample whose words are traced gives the
   slot's `TextureSource::sampler_words`. The host reads the words at the dispatch and binds that
   sampler for the slot, so the image is read with the guest's wrap and filter modes. A compare's
   function and filter are compiled in.
3. Neither names it. The sampler is still in the program's scalar registers at the sample, and the
   module reads the function and filter there as it runs: `DEPTH_COMPARE_FUNC`, word 0 bits 14:12,
   and `XY_MAG_FILTER`'s bilinear bit, word 2 bit 20 (`gfx10-rsrc.json`). It makes every
   function's answer and both filters' answers, and keeps the ones the sampler names. A format
   load reads its descriptor's format the same way (D967). `UserData::compared` says so with
   `SHADER_COMPARE_RUNNING`. The texels are then sampled through the host's default sampler, so
   wrap and filter modes come from the default. Only the comparison and its filter are the guest's.

**Why:** Grand Theft Auto V's shadow pass at `0x74005b858800` compares at `0x24f8` through
`s[24:27]`. Its image comes from the table at user data 2 and 3 (`s_load_dwordx16 s[8:23], s[2:3]`).
Its sampler is built from constants just before the sample: `s_movk_i32 s24, 0x3092`,
`s_bfm_b32 s25, 12, 12`, `s_mov_b32 s26, 0xa500000`, `s_mov_b32 s27, 0`. That is compare function 3,
clamped to the last texel, filtered bilinearly. An earlier `s_buffer_load_dwordx16 s[24:39]` loads
those registers, but it is overwritten before the sample. The image's own source named no sampler,
so the device-addressed route refused the dispatch with "compared image 9 names no sampler" in
every run after the selected-image work, and the pass drew nothing.

The sampler is constants, not something the program reads at run time. So the second way reads it
exactly: wrap, filter and compare are all the guest's.

**Considered:**
- A table of every sampler a buffer can name, bound as a sampler array and found through a
  directory on the sampler's words, as D983 binds images. No program met so far selects its sampler
  at run time. The one that seemed to does not. D983 already rejected this table until such a
  program says where the candidates lie, and that still holds. The third way above covers the
  comparison for a sampler the host cannot name. Wrap and filter for such a sampler are the known
  limit that would justify the table.
- Refusing until the host could trace a sampler read out of a buffer, as a descriptor would be
  (D934). That covers one load shape. The registers hold the answer for every shape.

**Tests:** `a_traced_sampler_is_read_from_its_words` and
`a_compare_through_an_unnamed_sampler_reads_it_as_it_runs` cover the host's choice. On the device:
`constant_samplers_wrap_and_filter_as_the_guest_built_them` samples one image through a
clamped point sampler and a repeating bilinear one, both built from constants, and checks every
lane's texel against the modes' arithmetic. `a_dispatch_s_compare_reads_its_sampler_as_it_runs`
answers every function, point and bilinear, from the sampler words in the registers.
