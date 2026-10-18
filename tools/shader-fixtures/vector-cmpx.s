// The compares into the execution mask no compiled fixture reaches: the float orderings, ordered
// and not, and the signed and unsigned ones the scene shaders mask lanes with.
//
// Grand Theft Auto V's scene pixel shaders narrow their lanes with them (VOPC opcodes 19 and 22
// had no recorded name).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names.
v_cmpx_lt_f32 0x43200000, v4
v_cmpx_eq_f32 0x43200000, v4
v_cmpx_le_f32 0x43200000, v4
v_cmpx_lg_f32 0x43200000, v4
v_cmpx_ge_f32 0x43200000, v4
v_cmpx_nge_f32 0x43200000, v4
v_cmpx_nlg_f32 0x43200000, v4
v_cmpx_ngt_f32 0x43200000, v4
v_cmpx_nle_f32 0x43200000, v4
v_cmpx_nlt_f32 0x43200000, v4
v_cmpx_lt_i32 5, v4
v_cmpx_ne_i32 5, v4
v_cmpx_ge_i32 5, v4
v_cmpx_le_u32 5, v4
v_cmpx_ge_u32 5, v4
s_endpgm
