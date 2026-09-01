# D141 - Sixty-four-lane wavefronts, and the tables do not care


**Status:** assumed

The retarget to RDNA2 (D139) reported 69 rejected probes on the first run. Two were real:
`v_mad_f32` does not exist on this generation, and `v_add_u32` is spelled
`v_add_nc_u32`. The other 67 were the reference assembler defaulting to **32-lane
wavefronts**, where `vcc` and `s[4:5]` are the wrong spelling for a mask.

This generation supports both widths, selected per wave. So it is a choice, and it is
recorded as `MATTR = "+wavefrontsize64"` in `orbistoun-gen`'s target module.

**What makes it cheap: the width does not change the encodings.** Checked, not assumed -
`v_cndmask_b32_e64 v0, v1, v2, s[4:5]` and its 32-lane spelling assemble to the same
bytes, with the same field holding the same 4. What differs is whether the mask that
field names is 32 or 64 bits wide. The width is therefore a property of the *shader*, not
of the tables, and generating in either mode produces the same table. The flag exists so
the assembler accepts the probes as written, and for no other reason.

**Why 64.** It is the width the translator already models throughout - `Fidelity::Wavefront`
is one invocation covering 64 lanes, and the execution mask is a 64-bit pair. And the
previous-generation console, the optional second target, has no other width.

**What this defers.** Real shaders on this generation will use 32-lane waves, and that is
a *translator* change - a mask width, and the invocation-to-lane ratio - reached when a
capture contains one. It is not a regeneration, and nothing here has to be revisited to
get there. Flagged `assumed` because nobody has yet seen which width the target's own
shaders are compiled for; the finding above is what makes being wrong cheap.

