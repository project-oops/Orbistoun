// The signed integer compares in their long and short forms, and the add-then-shift, which no
// compiled fixture reaches.
//
// Transform feedback's vertex shaders compare into an ordinary register pair and form addresses
// with an add then a shift (GCTS00001, KHR-GL30.transform_feedback.draw_xfb_test).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names. Vector, scalar and constant sources, and destination pairs spread across the file.
v_cmp_lt_i32_e64 s[2:3], 0, v0
v_cmp_eq_i32_e64 s[2:3], 0, v0
v_cmp_eq_i32_e64 s[40:41], v5, s7
v_cmp_le_i32_e64 s[4:5], v1, v2
v_cmp_gt_i32_e64 s[2:3], 0, v0
v_cmp_gt_i32_e64 s[90:91], v200, -1
v_cmp_ge_i32_e64 s[6:7], s8, v9
v_cmp_lt_i32_e32 vcc, 0, v0
v_cmp_le_i32_e32 vcc, s8, v9
v_cmp_gt_i32_e32 vcc, 4, v8
v_cmp_ge_i32_e32 vcc, v200, v255
v_add_lshl_u32 v2, v0, v1, 2
v_add_lshl_u32 v200, s7, v9, v130
s_endpgm
