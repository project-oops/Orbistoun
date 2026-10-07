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
v_xor_b32_e32 v8, v1, v6
v_xor_b32_e32 v250, s3, v9
v_min3_i16 v3, v4, v5, v6
v_cmpx_le_i16_e32 0, v2

// ---- A vertex fetch: radeonsi's 8- and 16-bit vertex elements, converted by the format the
// buffer's descriptor names (si_nir_lower_vs_inputs.c), and a whole constant block -------
buffer_load_format_x v1, v4, s[8:11], 0 idxen
buffer_load_format_xy v[2:3], v4, s[8:11], 0 idxen
buffer_load_format_xyz v[5:7], v4, s[12:15], 0 idxen
buffer_load_format_xyzw v[8:11], v4, s[16:19], 0 idxen
buffer_store_format_x v1, v0, s[4:7], 0 idxen
buffer_store_format_xy v[2:3], v0, s[4:7], 0 idxen
buffer_store_format_xyz v[5:7], v0, s[4:7], 0 idxen
buffer_store_format_xyzw v[8:11], v0, s[4:7], 0 idxen
s_buffer_load_dwordx16 s[32:47], s[4:7], 0x40
v_mad_f32 v12, v1, v2, v3
v_mac_f32_e32 v13, v1, v2
v_mul_u32_u24_e32 v14, 0x1234, v2

// ---- What ACO writes for a vertex shader's and a fragment shader's comparisons, selects and
// bitfields: float compares into a lane mask, a scalar select of one, a bitfield mask and insert,
// and an integer conversion -------------------------------------------------------------
v_cmp_eq_f32_e64 s[2:3], |v1|, v2
v_cmp_le_f32_e64 s[4:5], v1, s9
v_cmp_ge_f32_e64 s[6:7], -v3, 1.0
s_cselect_b64 s[8:9], -1, 0
s_bfm_b32 s4, 8, 23
v_bfi_b32 v15, v1, v2, v3
v_cvt_f32_i32_e32 v16, v1
// The multiply-adds with a constant ACO folds in (`aco_optimizer.cpp`'s madak/madmk): the
// constant K is a literal word after the instruction.
v_madak_f32 v17, v1, v2, 0xbdf0555d
// The AGC titles' vertex and pixel shaders: a no-op, and two floats packed to halves.
v_nop
v_cvt_pkrtz_f16_f32_e64 v18, v1, v2
// PPSA03416's vertex shader narrows its execution mask with a 64-bit shift.
s_lshr_b64 exec, -1, vcc_lo
s_lshl_b64 s[4:5], s[6:7], s8
v_madmk_f32 v18, v1, 0x3e800000, v2
// An unsigned scalar compare and a compare into the execution mask, as a pixel shader's discard
// test is written.
s_cmp_lg_u32 s2, 0
v_cmpx_neq_f32_e32 v1, v2

// ---- What ACO's culling primitive shader runs (ac_nir_lower_ngg.c): the vertices' and
// primitives' compaction through the local data share, bounding-box and facing tests, and the
// cross-lane reads of a wave's counts --------------------------------------------------------
ds_write2_b32 v1, v2, v3 offset0:4 offset1:9
ds_write_b8 v1, v2 offset:4
ds_read2_b32 v[5:6], v1 offset0:4 offset1:9
ds_or_b32 v1, v2 offset:4
ds_read_u8 v5, v1 offset:4
v_mad_u32_u16 v3, v4, v5, v6
v_max3_f32 v3, v4, v5, v6
v_min3_f32 v3, v4, v5, v6
v_cmp_lt_f32_e64 s[2:3], v4, v5
v_cmp_gt_f32_e64 s[2:3], v4, v5
v_cmp_neq_f32_e64 s[2:3], v4, v5
v_mad_i32_i24 v3, v4, v5, v6
v_permlane16_b32 v3, v4, s5, s6
v_msad_u8 v3, v4, v5, v6
v_readlane_b32 s3, v4, s5
v_mul_lo_u32 v3, v4, v5
v_and_or_b32 v3, v4, v5, v6
s_bfe_u64 s[4:5], s[6:7], s2
s_xor_b64 s[4:5], s[6:7], s[2:3]
s_nor_b64 s[4:5], s[6:7], s[2:3]
s_orn2_b64 s[4:5], s[6:7], s[2:3]
s_lshr_b32 s4, s6, s2
s_add_u32 s4, s6, s2
s_cselect_b32 s4, s6, s2
s_bitcmp1_b32 s6, s2
s_and_saveexec_b64 s[4:5], s[6:7]
s_bcnt1_i32_b64 s4, s[6:7]
v_rndne_f32_e32 v3, v1
v_bfrev_b32_e32 v3, v1
v_cmpx_gt_f32_e32 v1, v2
v_cmpx_eq_i32_e32 v1, v2
v_cmpx_gt_i32_e32 v1, v2
// The AGC formatted copy's bound: an unsigned count against the thread index (PPSA03416).
v_cmpx_gt_u32_e32 vcc_lo, v0
v_subrev_nc_u32_e32 v0, s4, v0
v_cmpx_le_i32_e32 0, v0
v_cmp_eq_i32_e32 vcc, 0, v0
v_floor_f32_e32 v3, v1
global_load_dwordx3 v[4:6], v8, s[6:7]
s_cmp_eq_u32 s7, 4
s_mov_b32 s102, s104
s_mov_b32 s105, s103
s_and_saveexec_b32 s54, vcc_lo
v_fract_f32_e32 v69, v67
s_andn2_b32 s54, s54, exec_lo

// ---- The copy itself: a load and a store, sixteen-bit address and data ------------
image_load v[6:7], v5, s[8:15] dmask:0xf dim:SQ_RSRC_IMG_2D unorm a16 d16
image_store v[6:7], v8, s[24:31] dmask:0xf dim:SQ_RSRC_IMG_2D unorm a16 d16
s_endpgm
