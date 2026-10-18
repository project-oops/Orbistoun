// The float compares no compiled fixture reaches: the ordered not-equal and the negated orderings,
// true for a NaN.
//
// Grand Theft Auto V's scene shaders test with them, plain and through SDWA (VOPC opcode 9 had no
// recorded name).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names. Each in its short form into vcc and its long form into a pair.
v_cmp_lg_f32 vcc, v0, v1
v_cmp_lg_f32_e64 s[4:5], v2, s3
v_cmp_nge_f32 vcc, 1.0, v128
v_cmp_nge_f32_e64 s[6:7], v4, 0.5
v_cmp_nlg_f32 vcc, s9, v10
v_cmp_nlg_f32_e64 s[100:101], v11, v12
v_cmp_nle_f32 vcc, v13, v14
v_cmp_nle_f32_e64 vcc, s15, v16
v_cmp_ngt_f32 vcc, v17, v18
v_cmp_ngt_f32_e64 s[20:21], v19, s22
s_endpgm
