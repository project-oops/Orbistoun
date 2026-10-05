// The opcodes radeonsi's compute image copy runs that no other fixture reaches.
//
// radeonsi copies an image with a compute program (`si_compute_copy_image`): each lane forms its
// texel's coordinate from the group id and its thread id in sixteen-bit halves, loads the texel,
// and stores it at the same place in the other image. ACO compiles the coordinate arithmetic to
// packed and sixteen-bit vector instructions, and the loads and stores to image instructions with
// sixteen-bit addresses and data.
//
// Written by hand and assembled, like `blit.s`: the reference decides the bytes and the
// boundaries. The `op_sel` forms ACO writes are not here - the reference does not assemble all of
// them - and the decoder reads that modifier the same whatever it holds. Nor is the SDWA move that
// sets a coordinate's high half: SDWA forms name their base opcode, as in `blit.s`.

// target triple: amdgcn-mesa-mesa3d

// ---- The coordinate, in sixteen-bit halves ----------------------------------------
s_mul_i32 s16, s16, s4
v_pk_mad_u16 v0, s16, s0, v0
v_pk_lshlrev_b16 v0, 1, v0
v_lshlrev_b16 v2, 1, v0
v_add_nc_u16 v4, v2, 1
v_add_nc_u16 v4, v5, v2
v_pack_b32_f16 v1, v2, v3

// ---- A texel's bytes rearranged: one format's bytes into another's ----------------
v_perm_b32 v1, v2, v3, s4

// ---- A copy between formats: a whole descriptor pair, and clamped halves ----------
s_load_dwordx16 s[16:31], s[2:3], 0x0
v_pk_sub_u16 v0, v1, v2
v_pk_add_u16 v0, v1, v2
v_pk_lshlrev_b16 v0, 0x20001, v0
v_min3_i16 v3, v4, v5, v6
v_cmpx_le_i16_e32 0, v2

// ---- The copy itself: a load and a store, sixteen-bit address and data ------------
image_load v[6:7], v5, s[8:15] dmask:0xf dim:SQ_RSRC_IMG_2D unorm a16 d16
image_store v[6:7], v8, s[24:31] dmask:0xf dim:SQ_RSRC_IMG_2D unorm a16 d16
s_endpgm
