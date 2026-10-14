// The three-source unsigned add, which no compiled fixture reaches.
//
// A legacy vertex shader's streamout pass forms its store address with it: the buffer's offset,
// the wave's first vertex and the lane's own, summed in one instruction (GCTS00001, D861).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names. Scalar, vector, constant and literal sources, and a high destination.
v_add3_u32 v0, s11, s10, v0
v_add3_u32 v1, v2, v3, v4
v_add3_u32 v200, 1, v9, s7
v_add3_u32 v5, v6, 0x1234, v7
s_endpgm
