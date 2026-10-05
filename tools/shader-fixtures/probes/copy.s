// radeonsi's compute image copy: the sixteen-bit arithmetic it forms its coordinates with, whose
// operands no other probe solves. The fixture `../copy.s` is where their names come from.
//
// Operands only. `op_sel`, which picks a source's or the destination's high half, is a modifier in
// bits 14:11 of the first word, and a varied sample of it would read as a field of its own.

// ---- s_mul_i32: SOP2, two sources --------------------------------------------------------
s_mul_i32 s16, s16, s4
s_mul_i32 s101, s5, s99
s_mul_i32 s0, -1, s77
s_mul_i32 s40, s3, 4
s_mul_i32 s9, 64, s62

// ---- v_pk_lshlrev_b16: VOP3P, a shift count then the value ------------------------------
v_pk_lshlrev_b16 v0, 1, v0
v_pk_lshlrev_b16 v200, v5, v9
v_pk_lshlrev_b16 v255, v12, s30
v_pk_lshlrev_b16 v9, s101, v77
v_pk_lshlrev_b16 v130, v88, 4

// ---- v_add_nc_u16: VOP3, two sources --------------------------------------------------
v_add_nc_u16 v4, v5, v2
v_add_nc_u16 v200, s5, v9
v_add_nc_u16 v255, v12, s30
v_add_nc_u16 v9, -1, v77
v_add_nc_u16 v130, v88, 4

// ---- v_lshlrev_b16: VOP3, a shift count then the value --------------------------------
v_lshlrev_b16 v2, 1, v0
v_lshlrev_b16 v200, s5, v9
v_lshlrev_b16 v255, v12, s30
v_lshlrev_b16 v9, -1, v77
v_lshlrev_b16 v130, v88, 4

// ---- v_pack_b32_f16: VOP3, two halves into one word -----------------------------------
v_pack_b32_f16 v1, v2, v3
v_pack_b32_f16 v200, s5, v9
v_pack_b32_f16 v255, v12, s30
v_pack_b32_f16 v9, -1, v77
v_pack_b32_f16 v130, v88, 4

// ---- v_pk_mad_u16: VOP3P, three sources ------------------------------------------------
v_pk_mad_u16 v0, s16, s0, v0
v_pk_mad_u16 v200, v5, v9, v190
v_pk_mad_u16 v255, v12, s30, v44
v_pk_mad_u16 v9, -1, v77, s101
v_pk_mad_u16 v130, v88, 4, v240

// ---- v_perm_b32: VOP3, two sources whose bytes are picked by a third --------------------
v_perm_b32 v1, v2, v3, s4
v_perm_b32 v200, s5, v9, v190
v_perm_b32 v255, v12, s30, 64
v_perm_b32 v9, -1, v77, v3
v_perm_b32 v130, v88, 4, s101

// ---- v_pk_sub_u16: VOP3P, two sources --------------------------------------------------
v_pk_sub_u16 v0, v1, v2
v_pk_sub_u16 v200, v5, v9
v_pk_sub_u16 v255, v12, s30
v_pk_sub_u16 v9, s101, v77
v_pk_sub_u16 v130, v88, 4

// ---- v_min3_i16: VOP3, three sources ----------------------------------------------------
v_min3_i16 v3, v4, v5, v6
v_min3_i16 v200, s5, v9, v190
v_min3_i16 v255, v12, s30, 64
v_min3_i16 v9, -1, v77, v3
v_min3_i16 v130, v88, 4, s101

// ---- s_load_dwordx16: SMEM, sixteen words. Destinations to the top of the file, bases past
// one hundred and offsets to the top of the field, as `memory.s` learned to.
s_load_dwordx16 s[16:31], s[2:3], 0x0
s_load_dwordx16 s[88:103], s[100:101], 0xfff0
s_load_dwordx16 s[48:63], s[86:87], 0x8000
s_load_dwordx16 s[0:15], s[70:71], 0x1234
s_load_dwordx16 s[64:79], s[36:37], 0x7ffc

// ---- v_cmpx_le_i16: VOPC, the execution mask from two sixteen-bit sources -------------
v_cmpx_le_i16_e32 0, v2
v_cmpx_le_i16_e32 v200, v130
v_cmpx_le_i16_e32 s101, v9
v_cmpx_le_i16_e32 -1, v255
v_cmpx_le_i16_e32 4, v77

// ---- v_pk_add_u16: VOP3P, two sources --------------------------------------------------
v_pk_add_u16 v0, v1, v2
v_pk_add_u16 v200, v5, v9
v_pk_add_u16 v255, v12, s30
v_pk_add_u16 v9, s101, v77
v_pk_add_u16 v130, v88, 4

// ---- buffer_load_format_x .. _xyzw: MUBUF, a vertex index into a descriptor. High and
// uncorrelated registers, as memory.s learned to, and a register soffset beside the inline zero.
buffer_load_format_x v1, v40, s[8:11], s3 idxen
buffer_load_format_x v37, v2, s[16:19], s13 idxen
buffer_load_format_x v200, v55, s[96:99], s101 idxen
buffer_load_format_x v60, v9, s[32:35], 0 idxen
buffer_load_format_x v130, v250, s[44:47], s43 idxen
buffer_load_format_xy v[1:2], v40, s[8:11], s3 idxen
buffer_load_format_xy v[37:38], v2, s[16:19], s13 idxen
buffer_load_format_xy v[200:201], v55, s[96:99], s101 idxen
buffer_load_format_xy v[60:61], v9, s[32:35], 0 idxen
buffer_load_format_xy v[130:131], v250, s[44:47], s43 idxen
buffer_load_format_xyz v[1:3], v40, s[8:11], s3 idxen
buffer_load_format_xyz v[37:39], v2, s[16:19], s13 idxen
buffer_load_format_xyz v[200:202], v55, s[96:99], s101 idxen
buffer_load_format_xyz v[60:62], v9, s[32:35], 0 idxen
buffer_load_format_xyz v[130:132], v250, s[44:47], s43 idxen
buffer_load_format_xyzw v[1:4], v40, s[8:11], s3 idxen
buffer_load_format_xyzw v[37:40], v2, s[16:19], s13 idxen
buffer_load_format_xyzw v[200:203], v55, s[96:99], s101 idxen
buffer_load_format_xyzw v[60:63], v9, s[32:35], 0 idxen
buffer_load_format_xyzw v[130:133], v250, s[44:47], s43 idxen

// ---- s_buffer_load_dwordx16: SMEM, sixteen words of a constant buffer, at the extremes.
s_buffer_load_dwordx16 s[32:47], s[4:7], 0x40
s_buffer_load_dwordx16 s[88:103], s[100:103], 0xfff0
s_buffer_load_dwordx16 s[48:63], s[84:87], 0x8000
s_buffer_load_dwordx16 s[0:15], s[68:71], 0x1234
s_buffer_load_dwordx16 s[64:79], s[36:39], 0x7ffc

// ---- v_mad_f32: VOP3, three sources, the legacy multiply-add ---------------------------
v_mad_f32 v3, v4, v5, v6
v_mad_f32 v200, s5, v9, v190
v_mad_f32 v255, v12, s30, 64
v_mad_f32 v9, -1, v77, v3
v_mad_f32 v130, v88, 4, s101

// ---- v_mac_f32 and v_mul_u32_u24: VOP2, two sources ------------------------------------
v_mac_f32_e32 v3, v4, v5
v_mac_f32_e32 v200, s5, v9
v_mac_f32_e32 v255, -1, v130
v_mac_f32_e32 v9, 4, v77
v_mac_f32_e32 v130, v88, v255
v_mul_u32_u24_e32 v3, v4, v5
v_mul_u32_u24_e32 v200, s5, v9
v_mul_u32_u24_e32 v255, -1, v130
v_mul_u32_u24_e32 v9, 4, v77
v_mul_u32_u24_e32 v130, v88, v255

// ---- v_cmp_eq/le/ge_f32_e64: VOP3-encoded compares into a lane mask, at the extremes ---------
v_cmp_eq_f32_e64 s[2:3], v4, v5
v_cmp_eq_f32_e64 s[100:101], s5, v190
v_cmp_eq_f32_e64 s[40:41], v255, -1
v_cmp_eq_f32_e64 vcc, 4, v77
v_cmp_eq_f32_e64 s[70:71], v130, s101
v_cmp_le_f32_e64 s[2:3], v4, v5
v_cmp_le_f32_e64 s[100:101], s5, v190
v_cmp_le_f32_e64 s[40:41], v255, -1
v_cmp_le_f32_e64 vcc, 4, v77
v_cmp_le_f32_e64 s[70:71], v130, s101
v_cmp_ge_f32_e64 s[2:3], v4, v5
v_cmp_ge_f32_e64 s[100:101], s5, v190
v_cmp_ge_f32_e64 s[40:41], v255, -1
v_cmp_ge_f32_e64 vcc, 4, v77
v_cmp_ge_f32_e64 s[70:71], v130, s101

// ---- s_cselect_b64 and s_bfm_b32: SOP2 --------------------------------------------------
s_cselect_b64 s[8:9], -1, 0
s_cselect_b64 s[100:101], s[2:3], s[40:41]
s_cselect_b64 s[0:1], 4, s[70:71]
s_cselect_b64 s[62:63], s[96:97], -16
s_bfm_b32 s4, 8, 23
s_bfm_b32 s101, s5, s90
s_bfm_b32 s0, -1, s33
s_bfm_b32 s60, s100, 4

// ---- v_bfi_b32: VOP3, three sources ----------------------------------------------------
v_bfi_b32 v3, v4, v5, v6
v_bfi_b32 v200, s5, v9, v190
v_bfi_b32 v255, v12, s30, 64
v_bfi_b32 v9, -1, v77, v3
v_bfi_b32 v130, v88, 4, s101

// ---- v_cvt_f32_i32: VOP1 ---------------------------------------------------------------
v_cvt_f32_i32_e32 v3, v4
v_cvt_f32_i32_e32 v200, s5
v_cvt_f32_i32_e32 v255, -1
v_cvt_f32_i32_e32 v9, 4
v_cvt_f32_i32_e32 v130, v255

// ---- v_madak_f32 and v_madmk_f32: VOP2 with a literal K after the instruction --------------
v_madak_f32 v3, v4, v5, 0xbdf0555d
v_madak_f32 v200, s5, v9, 0x3e800000
v_madak_f32 v255, -1, v130, 0x12345678
v_madak_f32 v9, 4, v77, 0x87654321
v_madak_f32 v130, v88, v255, 0x7f7fffff
v_madmk_f32 v3, v4, 0xbdf0555d, v5
v_madmk_f32 v200, s5, 0x3e800000, v9
v_madmk_f32 v255, -1, 0x12345678, v130
v_madmk_f32 v9, 4, 0x87654321, v77
v_madmk_f32 v130, v88, 0x7f7fffff, v255

// ---- v_cmp_ge/le_f32_e32: VOPC compares into vcc ---------------------------------------
v_cmp_ge_f32_e32 vcc, v4, v5
v_cmp_ge_f32_e32 vcc, s5, v190
v_cmp_ge_f32_e32 vcc, -1, v255
v_cmp_ge_f32_e32 vcc, 4, v77
v_cmp_ge_f32_e32 vcc, v130, v12
v_cmp_le_f32_e32 vcc, v4, v5
v_cmp_le_f32_e32 vcc, s5, v190
v_cmp_le_f32_e32 vcc, -1, v255
v_cmp_le_f32_e32 vcc, 4, v77
v_cmp_le_f32_e32 vcc, v130, v12

// ---- s_cmp_lg_u32: SOPC; v_cmpx_neq_f32_e32: VOPC into exec ------------------------------
s_cmp_lg_u32 s2, 0
s_cmp_lg_u32 s101, s5
s_cmp_lg_u32 -1, s90
s_cmp_lg_u32 s0, 64
s_cmp_lg_u32 s60, s100
v_cmpx_neq_f32_e32 v4, v5
v_cmpx_neq_f32_e32 s5, v190
v_cmpx_neq_f32_e32 -1, v255
v_cmpx_neq_f32_e32 4, v77
v_cmpx_neq_f32_e32 v130, v12

// ---- The culling primitive shader's instructions, at the extremes. A pair access prints an offset
// at zero not at all, so each prints both, or the second takes the first's place ---------------------------
ds_write2_b32 v1, v2, v3 offset0:4 offset1:9
ds_write2_b32 v200, v9, v255 offset0:1 offset1:255
ds_write2_b32 v0, v130, v77 offset0:255 offset1:2
ds_write2_b32 v77, v1, v2 offset0:128 offset1:64
ds_write2_b32 v255, v0, v200 offset0:12 offset1:200
ds_write_b8 v1, v2 offset:4
ds_write_b8 v200, v9
ds_write_b8 v0, v255 offset:65535
ds_write_b8 v77, v130 offset:257
ds_write_b8 v255, v0 offset:12
ds_read2_b32 v[5:6], v1 offset0:4 offset1:9
ds_read2_b32 v[200:201], v9 offset0:1 offset1:255
ds_read2_b32 v[0:1], v130 offset0:255 offset1:2
ds_read2_b32 v[254:255], v77 offset0:128 offset1:64
ds_read2_b32 v[77:78], v255 offset0:12 offset1:200
ds_or_b32 v1, v2 offset:4
ds_or_b32 v200, v9
ds_or_b32 v0, v255 offset:65535
ds_or_b32 v77, v130 offset:257
ds_or_b32 v255, v0 offset:12
ds_read_u8 v5, v1 offset:4
ds_read_u8 v200, v9
ds_read_u8 v0, v130 offset:65535
ds_read_u8 v255, v77 offset:257
ds_read_u8 v77, v255 offset:12
v_mad_u32_u16 v3, v4, v5, v6
v_mad_u32_u16 v200, s5, v9, v190
v_mad_u32_u16 v255, v12, s30, 64
v_mad_u32_u16 v9, -1, v77, v3
v_mad_u32_u16 v130, v88, 4, s101
v_max3_f32 v3, v4, v5, v6
v_max3_f32 v200, s5, v9, v190
v_max3_f32 v255, v12, s30, 64
v_max3_f32 v9, -1, v77, v3
v_max3_f32 v130, v88, 4, s101
v_min3_f32 v3, v4, v5, v6
v_min3_f32 v200, s5, v9, v190
v_min3_f32 v255, v12, s30, 64
v_min3_f32 v9, -1, v77, v3
v_min3_f32 v130, v88, 4, s101
v_cmp_lt_f32_e64 s[2:3], v4, v5
v_cmp_lt_f32_e64 s[100:101], s5, v190
v_cmp_lt_f32_e64 s[40:41], v255, -1
v_cmp_lt_f32_e64 vcc, 4, v77
v_cmp_lt_f32_e64 s[70:71], v130, s101
v_cmp_gt_f32_e64 s[2:3], v4, v5
v_cmp_gt_f32_e64 s[100:101], s5, v190
v_cmp_gt_f32_e64 s[40:41], v255, -1
v_cmp_gt_f32_e64 vcc, 4, v77
v_cmp_gt_f32_e64 s[70:71], v130, s101
v_cmp_neq_f32_e64 s[2:3], v4, v5
v_cmp_neq_f32_e64 s[100:101], s5, v190
v_cmp_neq_f32_e64 s[40:41], v255, -1
v_cmp_neq_f32_e64 vcc, 4, v77
v_cmp_neq_f32_e64 s[70:71], v130, s101
v_mad_i32_i24 v3, v4, v5, v6
v_mad_i32_i24 v200, s5, v9, v190
v_mad_i32_i24 v255, v12, s30, 64
v_mad_i32_i24 v9, -1, v77, v3
v_mad_i32_i24 v130, v88, 4, s101
v_permlane16_b32 v3, v4, s5, s6
v_permlane16_b32 v200, v9, s100, s101
v_permlane16_b32 v255, v130, s0, s33
v_permlane16_b32 v9, v77, s60, s2
v_permlane16_b32 v130, v255, s7, s90
v_msad_u8 v3, v4, v5, v6
v_msad_u8 v200, s5, v9, v190
v_msad_u8 v255, v12, s30, 64
v_msad_u8 v9, -1, v77, v3
v_msad_u8 v130, v88, 4, s101
v_readlane_b32 s3, v4, s5
v_readlane_b32 s100, v9, 63
v_readlane_b32 s0, v255, s33
v_readlane_b32 s60, v130, 4
v_readlane_b32 s7, v77, s90
v_mul_lo_u32 v3, v4, v5
v_mul_lo_u32 v200, s5, v9
v_mul_lo_u32 v255, v12, s30
v_mul_lo_u32 v9, -1, v77
v_mul_lo_u32 v130, v88, 4
v_and_or_b32 v3, v4, v5, v6
v_and_or_b32 v200, s5, v9, v190
v_and_or_b32 v255, v12, s30, 64
v_and_or_b32 v9, -1, v77, v3
v_and_or_b32 v130, v88, 4, s101

// ---- The culling primitive shader's scalar mask and count arithmetic, and its rounding,
// bit reversal and compares into the execution mask ------------------------------------------
s_bfe_u64 s[4:5], s[6:7], s2
s_bfe_u64 s[100:101], s[40:41], s90
s_bfe_u64 s[0:1], s[96:97], s7
s_bfe_u64 s[62:63], -1, s33
s_bfe_u64 s[8:9], s[2:3], 64
s_xor_b64 s[4:5], s[6:7], s[2:3]
s_xor_b64 s[100:101], s[40:41], s[90:91]
s_xor_b64 s[0:1], -1, s[96:97]
s_xor_b64 s[62:63], s[2:3], 64
s_xor_b64 s[8:9], 4, s[32:33]
s_nor_b64 s[4:5], s[6:7], s[2:3]
s_nor_b64 s[100:101], s[40:41], s[90:91]
s_nor_b64 s[0:1], -1, s[96:97]
s_nor_b64 s[62:63], s[2:3], 64
s_nor_b64 s[8:9], 4, s[32:33]
s_orn2_b64 s[4:5], s[6:7], s[2:3]
s_orn2_b64 s[100:101], s[40:41], s[90:91]
s_orn2_b64 s[0:1], -1, s[96:97]
s_orn2_b64 s[62:63], s[2:3], 64
s_orn2_b64 s[8:9], 4, s[32:33]
s_lshr_b32 s4, s6, s2
s_lshr_b32 s101, s40, s90
s_lshr_b32 s0, -1, s97
s_lshr_b32 s63, s2, 31
s_lshr_b32 s8, 4, s33
s_add_u32 s4, s6, s2
s_add_u32 s101, s40, s90
s_add_u32 s0, -1, s97
s_add_u32 s63, s2, 64
s_add_u32 s8, 4, s33
s_cselect_b32 s4, s6, s2
s_cselect_b32 s101, s40, s90
s_cselect_b32 s0, -1, s97
s_cselect_b32 s63, s2, 64
s_cselect_b32 s8, 4, s33
s_bitcmp1_b32 s6, s2
s_bitcmp1_b32 s40, s90
s_bitcmp1_b32 -1, s97
s_bitcmp1_b32 s2, 31
s_bitcmp1_b32 s101, 4
s_and_saveexec_b64 s[4:5], s[6:7]
s_and_saveexec_b64 s[100:101], s[40:41]
s_and_saveexec_b64 s[0:1], -1
s_and_saveexec_b64 s[62:63], vcc
s_and_saveexec_b64 s[8:9], 64
s_bcnt1_i32_b64 s4, s[6:7]
s_bcnt1_i32_b64 s101, s[40:41]
s_bcnt1_i32_b64 s0, -1
s_bcnt1_i32_b64 s63, vcc
s_bcnt1_i32_b64 s8, 64
v_rndne_f32_e32 v3, v1
v_rndne_f32_e32 v200, s5
v_rndne_f32_e32 v255, -1
v_rndne_f32_e32 v9, 4
v_rndne_f32_e32 v130, v255
v_bfrev_b32_e32 v3, v1
v_bfrev_b32_e32 v200, s5
v_bfrev_b32_e32 v255, -1
v_bfrev_b32_e32 v9, 4
v_bfrev_b32_e32 v130, v255
v_cmpx_gt_f32_e32 v4, v5
v_cmpx_gt_f32_e32 s5, v190
v_cmpx_gt_f32_e32 -1, v255
v_cmpx_gt_f32_e32 4, v77
v_cmpx_gt_f32_e32 v130, v12
v_cmpx_eq_i32_e32 v4, v5
v_cmpx_eq_i32_e32 s5, v190
v_cmpx_eq_i32_e32 -1, v255
v_cmpx_eq_i32_e32 4, v77
v_cmpx_eq_i32_e32 v130, v12
v_cmpx_gt_i32_e32 v4, v5
v_cmpx_gt_i32_e32 s5, v190
v_cmpx_gt_i32_e32 -1, v255
v_cmpx_gt_i32_e32 4, v77
v_cmpx_gt_i32_e32 v130, v12
