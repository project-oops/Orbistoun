// The three-source or, and the float compare "not less than", which no compiled fixture reaches.
//
// PPSA04263's scene shaders combine bit masks with the first and order-test a float with the
// second (VOP3 opcode 882 and VOPC opcode 14 had no recorded name).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names. Scalar, vector, constant and literal sources, and a high destination.
v_or3_b32 v19, v0, v1, v2
v_or3_b32 v3, s4, v5, 1
v_or3_b32 v200, v9, 0x1234, s7
v_cmp_nlt_f32 vcc, v0, v1
v_cmp_nlt_f32 vcc, 1.0, v128
v_cmp_nlt_f32_e64 s[4:5], v2, s3
s_endpgm
