// The signed minimum and maximum, the rounding float-to-integer conversions, the half-float
// conversion, the class compare, the relative move, the three- and four-word local writes, the
// local atomics, the buffer atomic add, the level-zero gather and the inverted save of the
// execution mask, which no compiled fixture reaches.
//
// PPSA04263's scene shaders use each of them (VOP2 opcodes 17 and 18, VOP1 10, 12, 13 and 67,
// VOPC 136, DS 7, 8, 32, 222 and 223, MUBUF 50, MIMG 71 and SOP1 55 had no recorded name), and its
// loading-screen reduction the signed scalar minimum, the two- and four-word local moves, the
// float local and buffer minimum and maximum (SOP2 6, DS 18, 19, 77, 118 and 255, MUBUF 63, 64),
// and the sixteen-bit buffer load its next program reads with (MUBUF 10).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names. Scalar, vector, constant and literal sources, and a high destination.
v_min_i32_e32 v0, v2, v0
v_min_i32_e32 v200, s5, v9
v_max_i32_e32 v1, -1, v3
v_max_i32_e32 v255, 0x1234, v130
v_cvt_rpi_i32_f32_e32 v0, v1
v_cvt_rpi_i32_f32_e32 v200, s4
v_cvt_flr_i32_f32_e32 v1, v2
v_cvt_flr_i32_f32_e32 v3, 0.5
v_cvt_f16_f32_e32 v0, v1
v_cvt_f16_f32_e32 v9, s7
v_cmp_class_f32_e32 vcc, v0, v1
v_cmp_class_f32_e32 vcc, s3, v4
v_movrels_b32_e32 v0, v1
ds_write_b96 v0, v[1:3]
ds_write_b96 v9, v[4:6] offset:16
ds_write_b128 v0, v[4:7]
ds_write_b128 v2, v[8:11] offset:32
ds_min_u32 v0, v1
ds_max_u32 v2, v3 offset:8
ds_add_rtn_u32 v0, v1, v2
buffer_atomic_add v0, v1, s[4:7], 0 offen
buffer_atomic_add v2, v3, s[8:11], s3 offen offset:16
image_gather4_lz v[0:3], v[4:5], s[0:7], s[8:11] dmask:0x1 dim:SQ_RSRC_IMG_2D
s_andn1_saveexec_b64 s[0:1], s[2:3]
s_andn1_saveexec_b64 s[4:5], vcc
s_min_i32 vcc_lo, s26, vcc_lo
s_min_i32 s0, s1, -2
ds_write_b64 v12, v[4:5] offset:16
ds_write_b64 v0, v[2:3]
ds_min_f32 v0, v9
ds_max_f32 v0, v6 offset:12
ds_read_b128 v[0:3], v4
ds_read_b64 v[4:5], v4 offset:16
buffer_atomic_fmin v0, v1, s[4:7], 0 offen
buffer_atomic_fmax v3, v2, s[4:7], 0 offen offset:12
buffer_load_ushort v0, v2, s[32:35], 0 idxen offset:2
buffer_load_ushort v1, v3, s[4:7], 0 offen
