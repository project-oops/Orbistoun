// The unsigned minimum and maximum, the 24-bit unsigned multiply-add and the three-word local
// read, which no compiled fixture reaches.
//
// PPSA04263's scene compute program clamps indices with the first two, forms a share address
// with the third and reads a three-word element with the fourth (VOP2 opcodes 19 and 20, VOP3
// opcode 323 and DS opcode 254 had no recorded name).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names. Scalar, vector, constant and literal sources, and a high destination.
v_min_u32_e32 v0, v2, v0
v_min_u32_e32 v200, s5, v9
v_min_u32_e32 v3, 0x1234, v4
v_max_u32_e32 v1, v1, v3
v_max_u32_e32 v255, -1, v130
v_max_u32_e32 v9, 4, v77
v_mad_u32_u24 v0, 12, v6, 0x600
v_mad_u32_u24 v19, v0, v1, v2
v_mad_u32_u24 v200, s4, v5, 1
ds_read_b96 v[0:2], v0
ds_read_b96 v[4:6], v9 offset:16
ds_read_b96 v[253:255], v200 offset:65535
s_endpgm
