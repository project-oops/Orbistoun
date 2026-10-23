// Grand Theft Auto V's instructions whose operand layouts the other probes do not solve: the
// shaders it runs in the prologue use them (D947 names them; this solves their operands).
// Every field is sampled at its top and at two other values, so no narrower window explains it.
//
// target triple: amdgcn-amd-amdhsa


// ---- SOP2: a scalar destination and two scalar sources ---------------------------------------
s_max_i32 s0, s1, s2
s_max_i32 s105, s103, s101
s_max_i32 s37, vcc_hi, -16
s_min_u32 s0, s1, s2
s_min_u32 s105, s103, s101
s_min_u32 s37, vcc_hi, -16
s_max_u32 s0, s1, s2
s_max_u32 s105, s103, s101
s_max_u32 s37, vcc_hi, -16

// ---- SOPK: a scalar and a sixteen-bit immediate ----------------------------------------------
s_cmpk_gt_i32 s0, 0x0
s_cmpk_gt_i32 s105, 0xffff
s_cmpk_gt_i32 s37, 0x1234
s_cmpk_ge_i32 s0, 0x0
s_cmpk_ge_i32 s105, 0xffff
s_cmpk_ge_i32 s37, 0x1234
s_cmpk_lt_i32 s0, 0x0
s_cmpk_lt_i32 s105, 0xffff
s_cmpk_lt_i32 s37, 0x1234
s_cmpk_le_i32 s0, 0x0
s_cmpk_le_i32 s105, 0xffff
s_cmpk_le_i32 s37, 0x1234

// ---- SOP1: a scalar destination and a scalar source ------------------------------------------
s_bitset1_b32 s0, s1
s_bitset1_b32 s105, s103
s_bitset1_b32 s37, -16
s_ff1_i32_b32 s0, s1
s_ff1_i32_b32 s105, s103
s_ff1_i32_b32 s37, -16
s_ff1_i32_b64 s0, s[2:3]
s_ff1_i32_b64 s105, s[100:101]
s_ff1_i32_b64 s37, s[40:41]
s_orn2_saveexec_b64 s[0:1], s[2:3]
s_orn2_saveexec_b64 s[104:105], s[100:101]
s_orn2_saveexec_b64 s[36:37], s[40:41]

// ---- SOPC: two scalar sources ----------------------------------------------------------------
s_cmp_lg_u64 s[0:1], s[2:3]
s_cmp_lg_u64 s[104:105], s[100:101]
s_cmp_lg_u64 s[36:37], -16

// ---- VOP1: a vector destination and a source -------------------------------------------------
v_ffbh_u32_e32 v0, v1
v_ffbh_u32_e32 v255, v255
v_ffbh_u32_e32 v37, s101
v_movreld_b32_e32 v0, v1
v_movreld_b32_e32 v255, v255
v_movreld_b32_e32 v37, s101
v_rcp_f16_e32 v0, v1
v_rcp_f16_e32 v255, v255
v_rcp_f16_e32 v37, s101

// ---- VOPC: a source and a vector, into vcc ---------------------------------------------------
v_cmp_u_f32_e32 vcc, v1, v2
v_cmp_u_f32_e32 vcc, v255, v255
v_cmp_u_f32_e32 vcc, s101, v37
v_cmp_lt_f16_e32 vcc, v1, v2
v_cmp_lt_f16_e32 vcc, v255, v255
v_cmp_lt_f16_e32 vcc, s101, v37
v_cmp_eq_f16_e32 vcc, v1, v2
v_cmp_eq_f16_e32 vcc, v255, v255
v_cmp_eq_f16_e32 vcc, s101, v37
v_cmp_le_f16_e32 vcc, v1, v2
v_cmp_le_f16_e32 vcc, v255, v255
v_cmp_le_f16_e32 vcc, s101, v37
v_cmp_gt_f16_e32 vcc, v1, v2
v_cmp_gt_f16_e32 vcc, v255, v255
v_cmp_gt_f16_e32 vcc, s101, v37
v_cmp_ge_f16_e32 vcc, v1, v2
v_cmp_ge_f16_e32 vcc, v255, v255
v_cmp_ge_f16_e32 vcc, s101, v37
v_cmp_neq_f16_e32 vcc, v1, v2
v_cmp_neq_f16_e32 vcc, v255, v255
v_cmp_neq_f16_e32 vcc, s101, v37

// ---- VOP3: three sources ---------------------------------------------------------------------
v_bfe_i32 v0, v1, v2, v3
v_bfe_i32 v255, v255, v255, v255
v_bfe_i32 v37, s101, v40, s5
v_alignbyte_b32 v0, v1, v2, v3
v_alignbyte_b32 v255, v255, v255, v255
v_alignbyte_b32 v37, s101, v40, s5
v_fma_f16 v0, v1, v2, v3
v_fma_f16 v255, v255, v255, v255
v_fma_f16 v37, s101, v40, s5
v_min3_f16 v0, v1, v2, v3
v_min3_f16 v255, v255, v255, v255
v_min3_f16 v37, s101, v40, s5
v_max3_f16 v0, v1, v2, v3
v_max3_f16 v255, v255, v255, v255
v_max3_f16 v37, s101, v40, s5
v_med3_f16 v0, v1, v2, v3
v_med3_f16 v255, v255, v255, v255
v_med3_f16 v37, s101, v40, s5

// ---- VOP3: two sources -----------------------------------------------------------------------
v_mul_hi_i32 v0, v1, v2
v_mul_hi_i32 v255, v255, v255
v_mul_hi_i32 v37, s101, v40
v_ldexp_f32 v0, v1, v2
v_ldexp_f32 v255, v255, v255
v_ldexp_f32 v37, s101, v40
v_bfm_b32 v0, v1, v2
v_bfm_b32 v255, v255, v255
v_bfm_b32 v37, s101, v40
v_bcnt_u32_b32 v0, v1, v2
v_bcnt_u32_b32 v255, v255, v255
v_bcnt_u32_b32 v37, s101, v40
v_lshrrev_b16 v0, v1, v2
v_lshrrev_b16 v255, v255, v255
v_lshrrev_b16 v37, s101, v40
v_sub_nc_i16 v0, v1, v2
v_sub_nc_i16 v255, v255, v255
v_sub_nc_i16 v37, s101, v40
v_writelane_b32 v0, s1, 0
v_writelane_b32 v255, s105, 31
v_writelane_b32 v37, s40, s5
v_permlanex16_b32 v0, v1, s2, s3
v_permlanex16_b32 v255, v255, s105, s103
v_permlanex16_b32 v37, v40, s5, s7

// ---- VOP3P: packed halves --------------------------------------------------------------------
v_pk_fma_f16 v0, v1, v2, v3
v_pk_fma_f16 v255, v255, v255, v255
v_pk_fma_f16 v37, s101, v40, s5
v_pk_add_f16 v0, v1, v2
v_pk_add_f16 v255, v255, v255
v_pk_add_f16 v37, s101, v40
v_pk_mul_f16 v0, v1, v2
v_pk_mul_f16 v255, v255, v255
v_pk_mul_f16 v37, s101, v40
v_pk_min_f16 v0, v1, v2
v_pk_min_f16 v255, v255, v255
v_pk_min_f16 v37, s101, v40
v_pk_max_f16 v0, v1, v2
v_pk_max_f16 v255, v255, v255
v_pk_max_f16 v37, s101, v40
v_pk_fmac_f16 v0, v1, v2
v_pk_fmac_f16 v255, v255, v255
v_pk_fmac_f16 v37, s101, v40
