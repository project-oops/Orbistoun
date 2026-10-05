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
