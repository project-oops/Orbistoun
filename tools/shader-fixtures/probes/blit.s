// radeonsi's blit shaders: the opcodes they run whose operands no other probe solves. The
// fixture `../blit.s` is where their names come from.
//
// target triple: amdgcn-mesa-mesa3d

// ---- s_setprio: a sixteen-bit immediate and nothing else ---------------------------
//
// As the other SOPP immediates (`primitive.s`): samples reach the top of the field so no
// narrower window explains them.
s_setprio 0
s_setprio 3
s_setprio 0x3fff
s_setprio 0xffff

// ---- s_waitcnt_depctr: a sixteen-bit counter mask and nothing else -----------------
//
// SOPP, as `s_setprio`. A mask the counter fields spell exactly prints as their names, which the
// solver does not read (0x0 and 0x1 both do), so every sample is one they do not.
s_waitcnt_depctr 0x7fff
s_waitcnt_depctr 0xffe3
s_waitcnt_depctr 0x1234
s_waitcnt_depctr 0xfffe
s_waitcnt_depctr 0xffff

// ---- s_waitcnt_vscnt: a destination and a sixteen-bit count ------------------------
//
// SOPK. radeonsi writes `null` as the register (code 125, in the same field); varied
// registers pin the destination field and varied counts the immediate. `null` itself is kept
// out: the solver matches printed operands to codes, and the name is not one it resolves.
s_waitcnt_vscnt s0, 0x0
s_waitcnt_vscnt s77, 0x3f
s_waitcnt_vscnt s5, 0x1234
s_waitcnt_vscnt s101, 0xffff
s_waitcnt_vscnt s40, 0x7fff

// ---- v_cmp_ne_i32_e64: a compare into a scalar pair --------------------------------
v_cmp_ne_i32_e64 s[2:3], 1, v5
v_cmp_ne_i32_e64 s[20:21], v7, s9
v_cmp_ne_i32_e64 s[100:101], v200, v255
v_cmp_ne_i32_e64 s[40:41], -1, v130
v_cmp_ne_i32_e64 vcc, s30, v12

// ---- The short forms whose mask is implicit ---------------------------------------
//
// `v_cmp_ge_u32_e32` writes and `v_cndmask_b32_e32` reads `vcc` with no field naming it; the
// solver records it as an implicit operand, as it does for the carry-in add.
v_cmp_ge_u32_e32 vcc, 1, v5
v_cmp_ge_u32_e32 vcc, v100, v200
v_cmp_ge_u32_e32 vcc, s30, v255
v_cmp_ge_u32_e32 vcc, -1, v77
v_cmp_ge_u32_e32 vcc, v190, v12
v_cndmask_b32_e32 v1, 1, v5, vcc
v_cndmask_b32_e32 v2, s15, v2, vcc
v_cndmask_b32_e32 v255, v200, v100, vcc
v_cndmask_b32_e32 v130, -1, v77, vcc
v_cndmask_b32_e32 v9, s101, v240, vcc

// ---- v_lshl_or_b32: three sources ------------------------------------------------
v_lshl_or_b32 v1, v1, 20, v0
v_lshl_or_b32 v200, s5, v9, v190
v_lshl_or_b32 v255, v12, s30, v44
v_lshl_or_b32 v9, -1, v77, s101
v_lshl_or_b32 v130, v88, 4, v240

// ---- image_load_mip: image_load with the level after the coordinate ----------------
//
// As `image.s`'s image_load: four operands, the resource a quarter-scale five-bit field, so
// resources above s60 pin its width; `unorm` sits just above `dmask`.
image_load_mip v[0:3], v[2:4], s[8:15] dmask:0xf dim:SQ_RSRC_IMG_2D unorm
image_load_mip v200, v[250:252], s[80:87] dmask:0x1 dim:SQ_RSRC_IMG_2D
image_load_mip v[100:101], v[2:4], s[40:47] dmask:0x3 dim:SQ_RSRC_IMG_2D
image_load_mip v[30:32], v[190:192], s[92:99] dmask:0x7 dim:SQ_RSRC_IMG_2D unorm
image_load_mip v[8:11], v[12:14], s[16:23] dmask:0xf dim:SQ_RSRC_IMG_2D

// ---- v_cmp_ne_i32_e32: the clear's other corner compare ---------------------------
//
// As v_cmp_ge_u32_e32 above: `vcc` implicit, the sources varied across the field.
v_cmp_ne_i32_e32 vcc, 1, v5
v_cmp_ne_i32_e32 vcc, v100, v200
v_cmp_ne_i32_e32 vcc, s30, v255
v_cmp_ne_i32_e32 vcc, -1, v77
v_cmp_ne_i32_e32 vcc, v190, v12

// ---- s_buffer_load_dword*: a destination, a descriptor quad and a byte offset -------
//
// As `memory.s`'s s_load samples, with a four-register descriptor in place of the address
// pair: high destinations and bases pin the field widths, and offsets reaching 0x3fc the
// immediate's. The clear's own `null` offset is not a name the solver resolves.
s_buffer_load_dword s5, s[8:11], 0x10
s_buffer_load_dword s9, s[12:15], 0x24
s_buffer_load_dword s100, s[96:99], 0x3fc
s_buffer_load_dword s33, s[68:71], 0x94
s_buffer_load_dword s21, s[20:23], 0x1c8
s_buffer_load_dwordx2 s[2:3], s[8:11], 0x40
s_buffer_load_dwordx2 s[12:13], s[16:19], 0x88
s_buffer_load_dwordx2 s[60:61], s[12:15], 0x2c
s_buffer_load_dwordx2 s[98:99], s[92:95], 0x3f8
s_buffer_load_dwordx4 s[8:11], s[20:23], 0x30
s_buffer_load_dwordx4 s[40:43], s[60:63], 0x140
s_buffer_load_dwordx4 s[16:19], s[4:7], 0x8
s_buffer_load_dwordx4 s[96:99], s[88:91], 0x3f0
s_buffer_load_dwordx8 s[24:31], s[36:39], 0x60
s_buffer_load_dwordx8 s[48:55], s[8:11], 0xc4
s_buffer_load_dwordx8 s[88:95], s[96:99], 0x3e0
