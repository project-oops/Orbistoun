// Some names moved between architecture generations and the old spellings do not
// assemble here: the unsigned add is `v_add_nc_u32`, the multiply-add is `v_fma_f32`,
// and the carry-in forms are `v_add_co_ci_u32` / `v_sub_co_ci_u32`. Renamed on this
// generation rather than removed - the arithmetic is the same (D139).
// Three-operand vector ALU. Each instruction appears several times with distinct
// registers so a field can be told apart from its neighbours: with one sample any
// field reading the right value explains it, and only varied samples eliminate the
// coincidences.
v_fma_f32 v1, v2, v3, v4
v_fma_f32 v11, v12, v13, v14
v_fma_f32 v21, v22, v23, v24
v_fma_f32 v5, v6, v7, v8
v_fma_f32 v15, v16, v17, v18
v_mul_f32_e64 v3, v4, v5
v_mul_f32_e64 v9, v10, v11
v_mul_f32_e64 v19, v20, v21
v_add_f32_e64 v2, v6, v7
v_add_f32_e64 v12, v16, v17

// High register numbers, to pin field *width*. With only low registers a narrower
// field explains the data just as well as the real one, and the solver would pick the
// narrow one - correct on every sample it was given and wrong on the first shader that
// uses a high register.
v_fma_f32 v200, v201, v202, v203
v_mul_f32_e64 v199, v198, v197
v_add_f32_e64 v255, v254, v253

v_mul_f32_e64 v5, 0, v6
v_mul_f32_e64 v7, 1.0, v8
v_fma_f32 v9, 0, v10, 1.0

// Deliberately non-consecutive and non-monotonic.
//
// Consecutive registers are the worst possible samples: with v5, v6, v7, v8 in adjacent
// fields, a field shifted slightly reads values that differ by the same constant and
// looks perfectly consistent. The solver picked one of those and was wrong on the first
// real shader. Spread and disorder are what make a coincidence impossible to sustain.
v_fma_f32 v1, v33, v7, v200
v_fma_f32 v50, v2, v99, v13
v_fma_f32 v170, v9, v240, v4
v_fma_f32 v70, v11, v250, v5
v_fma_f32 v3, v128, v9, v61
v_fma_f32 v222, v17, v6, v190
v_mul_f32_e64 v40, v130, v3
v_mul_f32_e64 v6, v77, v210
v_add_f32_e64 v99, v14, v233
v_add_f32_e64 v180, v250, v37

// A constant in every operand position, for every opcode.
//
// Without one, a field is indistinguishable from a direct register index: samples that
// only ever hold a register cannot tell "register 242" from "the constant 1.0", and
// those decode differently. Each position needs at least one sample reaching outside
// the register range.
v_fma_f32 v1, 1.0, v3, v4
v_fma_f32 v2, v5, 0, v6
v_fma_f32 v3, v7, v8, 1.0
v_fma_f32 v4, 2.0, v9, v10
v_fma_f32 v5, v11, 4.0, v12
v_fma_f32 v6, v13, v14, 0
v_mul_f32_e64 v7, 1.0, v15
v_mul_f32_e64 v8, v16, 0
v_add_f32_e64 v9, 2.0, v17
v_add_f32_e64 v10, v18, 1.0

// ---- VOPC: comparisons, which is where a mask comes from ------------------------
//
// The 32-bit form writes vcc implicitly, so the destination is not an operand and only
// the two sources are. That asymmetry is the point: a per-family layout could not say
// "this one has an invisible destination", which is why layouts are per opcode (D096).
//
// Each source gets its own high sample, because the field that did not get one solves
// too narrow - four times now.
v_cmp_lt_f32_e32 vcc, v0, v1
v_cmp_lt_f32_e32 vcc, v100, v200
v_cmp_lt_f32_e32 vcc, s30, v255
v_cmp_lt_f32_e32 vcc, 0, v77
v_cmp_lt_f32_e32 vcc, v190, v12
v_cmp_eq_f32_e32 vcc, v2, v3
v_cmp_eq_f32_e32 vcc, v255, v130
v_cmp_eq_f32_e32 vcc, 1.0, v44
v_cmp_eq_f32_e32 vcc, s101, v9
v_cmp_gt_f32_e32 vcc, v5, v6
v_cmp_gt_f32_e32 vcc, v200, v255
v_cmp_gt_f32_e32 vcc, -1, v88
v_cmp_gt_f32_e32 vcc, s70, v240

// ---- v_mbcnt: where a lane learns its own index ---------------------------------
//
// Counts the set bits of a mask below this lane and adds the second source. With the
// mask all ones that is the lane index, which is how a shader gets one - there is no
// "lane id" instruction, and the value is not handed to the shader either.
//
// It is the instruction that makes divergence possible at all: without a per-lane value
// every comparison compares the same two registers in every lane, and every mask is
// all-ones or all-zero.
//
// Each source gets its own high sample.
v_mbcnt_lo_u32_b32 v0, -1, 0
v_mbcnt_lo_u32_b32 v100, s5, v200
v_mbcnt_lo_u32_b32 v255, v190, 0
v_mbcnt_lo_u32_b32 v12, 0, v255
v_mbcnt_lo_u32_b32 v44, s101, v77
v_mbcnt_hi_u32_b32 v1, -1, v0
v_mbcnt_hi_u32_b32 v200, s30, v255
v_mbcnt_hi_u32_b32 v255, v88, 0
v_mbcnt_hi_u32_b32 v9, 0, v190
v_mbcnt_hi_u32_b32 v130, s70, v240

// ---- integer vector work: addresses and unsigned comparisons ---------------------
//
// A shader computes an address from a lane index and compares indices against bounds,
// and neither is float work. `v_lshlrev_b32` takes its shift *first* and the value
// second - the reverse of how it reads - which is a mistake that produces a plausible
// wrong address rather than a failure.
v_add_nc_u32_e32 v1, v2, v3
v_add_nc_u32_e32 v100, s5, v200
v_add_nc_u32_e32 v255, v190, v12
v_add_nc_u32_e32 v44, 0, v77
v_add_nc_u32_e32 v9, s101, v255
v_lshlrev_b32_e32 v1, 2, v0
v_lshlrev_b32_e32 v200, s30, v255
v_lshlrev_b32_e32 v255, v88, v190
v_lshlrev_b32_e32 v9, -1, v240
v_lshlrev_b32_e32 v130, s70, v77
v_cmp_lt_u32_e32 vcc, v0, v1
v_cmp_lt_u32_e32 vcc, v100, v200
v_cmp_lt_u32_e32 vcc, s30, v255
v_cmp_lt_u32_e32 vcc, 0, v77
v_cmp_lt_u32_e32 vcc, v190, v12

// ---- v_cndmask_b32 and the long-form subtracts -----------------------------------
//
// cndmask takes a sixty-four-bit mask as its third source, named by its low register -
// so that field admits a scaled reading and the samples have to separate them. The
// long-form subtracts are here because they are in the supported list and a supported
// instruction with no operand layout is refused at translation, which reads as an
// unimplemented instruction rather than as a missing probe.
v_cndmask_b32_e64 v0, v1, v2, s[4:5]
v_cndmask_b32_e64 v100, v200, v255, s[70:71]
v_cndmask_b32_e64 v255, v130, v44, s[100:101]
v_cndmask_b32_e64 v9, v77, v190, vcc
v_cndmask_b32_e64 v44, v255, v12, s[36:37]
v_sub_f32_e64 v0, v1, v2
v_sub_f32_e64 v100, s30, v255
v_sub_f32_e64 v255, v190, s101
v_sub_f32_e64 v12, -1, v77
v_subrev_f32_e64 v1, v2, v3
v_subrev_f32_e64 v200, s70, v255
v_subrev_f32_e64 v255, v88, s101
v_subrev_f32_e64 v9, 0, v190

// Every cndmask sample above used a vector register for its first source, so an 8-bit
// direct index and a 9-bit shared-numbering reading both explained all of them - and
// those decode differently, so the solver refused rather than picking. A field holding
// 128 is v128 under one reading and the inline constant zero under the other.
//
// Separated by sources that are not vector registers: only the shared numbering can
// express a scalar register or an inline constant.
// A *scalar* first source is not available here: the long form may read one value off
// the constant bus per instruction and the mask has already taken it, so
// `v_cndmask_b32_e64 v0, s1, v2, s[4:5]` is rejected by the assembler. Inline constants
// are exempt from that restriction, which is what makes this separable at all - and it
// is worth knowing that a legal instruction was the only tool available.
v_cndmask_b32_e64 v1, 0, v3, vcc
v_cndmask_b32_e64 v2, -1, v4, s[8:9]
v_cndmask_b32_e64 v3, 1.0, v5, vcc
// And the same again for the second source, which had the identical ambiguity for the
// identical reason.
v_cndmask_b32_e64 v4, v6, 0, vcc
v_cndmask_b32_e64 v5, v7, -1, s[12:13]
v_cndmask_b32_e64 v6, v8, 1.0, vcc

// ---- VOP3b: the sub-encoding with a scalar destination ---------------------------
//
// These carry a *second* destination - a carry-out or a scaling flag - in bits 8 to 14
// of the first word. VOP3a uses those same bits for the per-source absolute-value flags,
// so reading them without knowing which sub-encoding an opcode uses turns a carry-out
// register into a set of modifiers. `vcc` as the scalar destination is 106, whose low
// three bits are 010 - so it would present as "the second source is an absolute value".
//
// The encoding table already says the family has two sub-encodings and claims no layout
// because of it; this is what that costs at the next layer up.
v_add_co_u32_e64 v0, vcc, v1, v2
v_add_co_u32_e64 v100, s[8:9], v200, v255
v_add_co_u32_e64 v255, s[100:101], v190, v12
v_add_co_u32_e64 v44, vcc, 0, v77
v_sub_co_u32_e64 v1, vcc, v2, v3
v_sub_co_u32_e64 v200, s[70:71], v255, v88
v_sub_co_u32_e64 v9, s[100:101], -1, v190
v_add_co_ci_u32_e64 v2, vcc, v3, v4, vcc
v_add_co_ci_u32_e64 v130, s[12:13], v200, v255, s[8:9]
v_add_co_ci_u32_e64 v77, s[100:101], v12, v44, s[70:71]
v_div_scale_f32 v0, vcc, v1, v2, v3
v_div_scale_f32 v100, s[8:9], v200, v255, v190
v_div_scale_f32 v255, s[100:101], v12, v44, v77
v_div_fmas_f32 v0, v1, v2, v3
v_div_fmas_f32 v100, v200, v255, v190
v_div_fmas_f32 v255, 1.0, v12, v44
v_div_fixup_f32 v1, v2, v3, v4
v_div_fixup_f32 v200, v255, v190, v12
v_div_fixup_f32 v255, -1, v77, v88

// Every sample above used a vector register for the later sources, so an eight-bit
// direct index and a nine-bit shared-numbering reading both explained all of them - the
// same ambiguity `v_cndmask_b32` hit, and the same cure. One inline constant per source
// position, because separating one slot says nothing about the next.
v_add_co_u32_e64 v0, vcc, v1, 0
v_add_co_u32_e64 v3, s[8:9], v4, -1
v_sub_co_u32_e64 v1, vcc, v2, 0
v_sub_co_u32_e64 v5, s[12:13], v6, -1
v_add_co_ci_u32_e64 v2, vcc, v3, 0, vcc
v_add_co_ci_u32_e64 v7, s[20:21], v8, -1, s[8:9]
v_div_scale_f32 v0, vcc, v1, 1.0, v3
v_div_scale_f32 v4, s[8:9], v5, v6, 1.0
v_div_scale_f32 v7, vcc, 1.0, v8, v9
v_div_fmas_f32 v0, v1, 1.0, v3
v_div_fmas_f32 v4, v5, v6, 1.0
v_div_fmas_f32 v7, 1.0, v8, v9
v_div_fixup_f32 v0, v1, -1, v3
v_div_fixup_f32 v4, v5, v6, 0
v_div_fixup_f32 v7, -1, v8, v9
// The carry-in form has five operands, and its first source had no inline constant
// among them - separating one slot says nothing about the next, five times over.
v_add_co_ci_u32_e64 v9, vcc, 0, v10, vcc
v_add_co_ci_u32_e64 v11, s[24:25], -1, v12, s[8:9]

// The short-form float subtracts. Supported by the translator and absent from every
// probe, so they had no recorded mnemonic - which matters now that translation is keyed
// on the name rather than the opcode number.
v_sub_f32_e32 v1, v2, v3
v_sub_f32_e32 v100, s30, v255
v_sub_f32_e32 v255, -1, v190
v_subrev_f32_e32 v2, v3, v4
v_subrev_f32_e32 v200, s70, v255
v_subrev_f32_e32 v9, 0, v88

// ---- the short forms that fell back to a family layout ---------------------------
//
// These were solved by the *family* layout rather than per opcode, so the solver had no
// entry for them and therefore no recorded name. That was invisible until translation
// started dispatching on names: a family layout is a fallback for the families whose
// shape genuinely is uniform, and it does not carry an opcode's identity with it.
v_mov_b32_e32 v0, v1
v_mov_b32_e32 v255, s101
v_mov_b32_e32 v100, -1
v_add_f32_e32 v0, v1, v2
v_add_f32_e32 v255, s30, v190
v_add_f32_e32 v9, -1, v77
v_mul_f32_e32 v1, v2, v3
v_mul_f32_e32 v200, s70, v255
v_mul_f32_e32 v12, 0, v88
v_rcp_f32_e32 v0, v1
v_rcp_f32_e32 v255, s101
v_rcp_f32_e32 v130, -1
