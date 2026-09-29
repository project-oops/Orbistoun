// The opcodes radeonsi's blit shaders run that no other fixture reaches.
//
// radeonsi draws a texture upload, a copy or a clear as a rectangle: a primitive shader that
// places three corners from user SGPRs (si_nir_lower_vs_inputs.c:97-142) and a pixel shader
// that fetches or writes one texel per pixel. ACO compiles them for the NGG geometry stage, so
// the primitive shader reads the geometry engine's counts out of its system SGPRs with bit-field
// extracts, packs its vertex indices, and selects each corner with compares and conditional
// moves; the pixel shader converts its interpolated position to an integer texel and loads it at
// a mip level.
//
// Written by hand and assembled, like `primitive.s`: the reference decides the bytes and the
// boundaries. The SDWA forms the primitive shader packs its indices with are not here - their
// operands print as modifiers the differential comparison does not model - and the decoder's
// own unit test holds their length.

// target triple: amdgcn-mesa-mesa3d

// ---- Scheduling: a priority hint and a workgroup barrier ---------------------------
s_setprio 3
s_barrier
s_setprio 0

// ---- A dependency-counter wait ACO places after a hazard, in a pixel shader --------
s_waitcnt_depctr 0xffe3
s_waitcnt_depctr 0xfffe

// ---- The geometry engine's counts, out of the system SGPRs -------------------------
s_bfe_u32 s0, s2, 0x90016
s_bfe_u32 s2, s2, 0x9000c
s_lshl_b32 s0, s0, 12
s_lshl_b32 s0, s3, 8
s_bfe_u32 exec_lo, -1, s0
s_bfm_b64 s[0:1], s3, 0
s_bfm_b64 s[40:41], 5, s7

// ---- Corner selection: 16-bit halves, compares, conditional moves ------------------
s_pack_ll_b32_b16 s0, s11, 0
s_pack_ll_b32_b16 s1, s10, s9
s_pack_hh_b32_b16 s10, s10, 0
s_pack_hh_b32_b16 s11, s12, s13
v_cmp_ge_u32_e32 vcc, 1, v5
v_cmp_ge_u32_e32 vcc, s4, v200
v_cmp_ne_i32_e64 s[2:3], 1, v5
v_cmp_ne_i32_e64 s[20:21], v7, s9
v_cndmask_b32_e32 v1, 1, v5, vcc
v_cndmask_b32_e32 v2, s15, v2, vcc
v_cndmask_b32_e32 v255, v200, v100, vcc
v_cvt_f32_u32_e32 v0, v0
v_cvt_f32_u32_e32 v200, s9

// ---- Index packing ---------------------------------------------------------------
v_or_b32_e32 v0, v0, v2
v_or_b32_e32 v250, s3, v9
v_lshl_or_b32 v1, v1, 20, v0
v_lshl_or_b32 v200, s5, v9, 0x3ff

// ---- The pixel shader: an integer texel, loaded at a level ------------------------
v_cvt_i32_f32_e32 v2, v0
v_cvt_i32_f32_e32 v130, s40
image_load_mip v[0:3], v[2:4], s[8:15] dmask:0xf dim:SQ_RSRC_IMG_2D unorm
image_load_mip v[4:5], v[6:8], s[16:23] dmask:0x3 dim:SQ_RSRC_IMG_2D unorm
s_waitcnt_vscnt null, 0x0
s_waitcnt_vscnt null, 0x3

// ---- The clear: a not-equal corner select, and the colour from a constant buffer ---
//
// radeonsi's clear primitive shader selects a corner with a signed not-equal compare as well
// as the unsigned one; its pixel shader builds a raw buffer descriptor over the colour from its
// user SGPRs and reads all four channels with one scalar buffer load. The shader itself has no
// offset (`null`); these carry immediates, which the comparison prints the same way it decodes.
v_cmp_ne_i32_e32 vcc, 1, v5
v_cmp_ne_i32_e32 vcc, s4, v200
s_buffer_load_dword s4, s[0:3], 0x8
s_buffer_load_dwordx2 s[4:5], s[8:11], 0x10
s_buffer_load_dwordx4 s[0:3], s[0:3], 0x0
s_buffer_load_dwordx8 s[8:15], s[4:7], 0x20
s_endpgm
