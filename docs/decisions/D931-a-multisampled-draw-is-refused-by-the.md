# D931 - A multisampled draw is named by the register that asked for it

**Status:** decided
**Date:** 2026-10-20

Every host target is drawn at one sample. A guest draw can ask for several, through
`PA_SC_AA_CONFIG.MSAA_NUM_SAMPLES`, a bound colour target's `CB_COLORn_ATTRIB.NUM_SAMPLES` and
`NUM_FRAGMENTS`, or the depth target's `DB_Z_INFO.NUM_SAMPLES`. `orbistoun-gpu`'s `msaa` module
reads these registers from the draw's state, together with `DB_EQAA`, `CB_COLOR_CONTROL.MODE`,
`PA_SC_AA_SAMPLE_LOCS_*`, `PA_SC_AA_MASK_*` and `PA_SC_MODE_CNTL_1.PS_ITER_SAMPLE`. Fields and
places come from Mesa's `gfx103.json`, and their use in a driver from radv. The module says how
the host would draw the draw: at one sample, or at a count with the guest's positions, coverage
mask and per-sample shading.

Every draw that is not at one sample is counted in its submission's report
(`multisampled_draws`). The count keeps the first draw's reason, which names the register, the
field and its value: for example `PA_SC_AA_CONFIG.MSAA_NUM_SAMPLES asks for 4 samples, which are
drawn at one`, or `CB_COLOR1_ATTRIB stores 2 fragments of 8 samples: EQAA is not modelled`. Each
distinct reason is logged once a run. The draw itself is still drawn at one sample.

Some shapes stay unmodelled even once targets are multisampled, because their console behaviour
is unmeasured. These are:

- a count of 16
- EQAA, where there are fewer fragments than samples or fewer depth anchors than samples
- exposed samples that differ from the rasterized ones
- a coverage mask that differs across the 2x2 quad
- a colour mode other than normal or disabled, including `CB_RESOLVE`
- a multisampled draw whose stream never wrote its sample positions

**Why:** a draw at one sample into a target the guest laid out at several writes back memory in
the wrong layout. Naming the register shows which titles draw multisampled, and with what, so the
work is ranked by them. It is also what the request to obSCEne (`REQ-20261020T1415Z-ms01`)
measures against.

**Rejected:**
- Refusing such a submission, so that it is neither drawn nor written back. Titles that show a
  picture today through a multisampled target would lose it with nothing measured. Whether a
  title's draws are refused or drawn at the guest's sample count is decided once the count shows
  where multisampling is used.
- Taking radv's default sample positions for a stream that wrote none. They are a driver's
  choice, not the console's. The library's context reset may write others.
