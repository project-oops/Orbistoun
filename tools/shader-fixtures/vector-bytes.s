// The byte conversions, the scalar or-not, the unsigned compares into the execution mask and the
// vector bit-field extract, which no compiled fixture reaches.
//
// Grand Theft Auto V's scene compute program unpacks bytes and masks lanes with them (SOP2 opcode
// 0x16, VOPC 0xc4, 0xd1, 0xd2 and 0xd5, VOP1 0x11 to 0x14 and VOP3 0x148 had no recorded name).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names.
s_orn2_b32 s8, vcc_lo, exec_lo
s_orn2_b32 s100, s3, 0x1234
v_cmp_gt_u32 vcc, vcc_lo, v0
v_cmp_gt_u32_e64 s[4:5], v2, s3
v_cmpx_lt_u32 vcc_hi, v2
v_cmpx_eq_u32 2, v1
v_cmpx_ne_u32 -1, v31
v_cvt_f32_ubyte0 v4, v2
v_cvt_f32_ubyte1 v3, s2
v_cvt_f32_ubyte2 v200, v2
v_cvt_f32_ubyte3 v2, 0x12345678
v_bfe_u32 v1, v0, 16, 8
v_bfe_u32 v200, s7, v9, v130
s_endpgm
