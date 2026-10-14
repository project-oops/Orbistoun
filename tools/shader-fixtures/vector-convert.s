// The integer and float conversions in their long form, which no compiled fixture reaches.
//
// A vertex shader converting a fetched integer attribute to float emits the VOP3 form when the
// compiler places it beside long-form neighbours (GCTS00001, KHR-GL30.clip_distance.functional).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names. Vector, scalar and constant sources, and a high destination.
v_cvt_f32_i32_e64 v5, v2
v_cvt_f32_i32_e64 v200, s7
v_cvt_f32_u32_e64 v5, v130
v_cvt_f32_u32_e64 v9, 1
v_cvt_i32_f32_e64 v4, v3
v_cvt_i32_f32_e64 v255, s30
v_cvt_u32_f32_e64 v6, v7
v_cvt_u32_f32_e64 v130, 1.0
s_endpgm
