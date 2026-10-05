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
