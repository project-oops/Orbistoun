// Grand Theft Auto V's in-world compute programs' instructions whose operand layouts the other
// probes do not solve: the lane permute through the local data share's crossbar, the 64-bit and
// one-word buffer atomics its scans publish and poll with, the local share's add that answers
// nothing, the long-form complement and the funnel shift; the local share's paired 64-bit write
// and read; and the cache invalidations and the rest of the one-word buffer accesses, so that a
// program using one is refused by its name rather than for an operand nothing laid out.
// Every field is sampled at its top and at two other values, so no narrower window explains it.
//
// target triple: amdgcn-amd-amdhsa

// ---- DS: a destination, the byte address of the lane to read, the value, an offset ------------
ds_bpermute_b32 v1, v2, v3 offset:4
ds_bpermute_b32 v255, v128, v77 offset:65535
ds_bpermute_b32 v0, v255, v255
ds_bpermute_b32 v77, v9, v128 offset:257
ds_permute_b32 v1, v2, v3 offset:4
ds_permute_b32 v255, v128, v77 offset:65535
ds_permute_b32 v0, v255, v255
ds_permute_b32 v77, v9, v128 offset:257

// ---- MUBUF: data pair, address, descriptor, scalar offset --------------------------------------
buffer_atomic_swap_x2 v[2:3], v40, s[8:11], s3 offen
buffer_atomic_swap_x2 v[254:255], v255, s[100:103], s105 offen
buffer_atomic_swap_x2 v[128:129], v0, s[64:67], s101 idxen
buffer_atomic_swap_x2 v[17:18], v128, s[36:39], s0 offen glc
buffer_atomic_or_x2 v[2:3], v40, s[8:11], s3 offen
buffer_atomic_or_x2 v[254:255], v255, s[100:103], s105 offen
buffer_atomic_or_x2 v[128:129], v0, s[64:67], s101 idxen
buffer_atomic_or_x2 v[17:18], v128, s[36:39], s0 offen glc

// ---- VOP3: the bitwise complement in its long form, a destination and one source -------------
v_not_b32_e64 v1, v2
v_not_b32_e64 v255, v255
v_not_b32_e64 v77, s105
v_not_b32_e64 v128, v130
v_not_b32_e64 v0, -1

// ---- MUBUF: the one-word or, as the pairs above ------------------------------------------------
buffer_atomic_or v2, v40, s[8:11], s3 offen
buffer_atomic_or v255, v255, s[100:103], s105 offen
buffer_atomic_or v128, v0, s[64:67], s101 idxen
buffer_atomic_or v17, v128, s[36:39], s0 offen glc

// ---- DS: the add that answers nothing, an address, a value and an offset -----------------------
ds_add_u32 v1, v2 offset:4
ds_add_u32 v255, v77 offset:65535
ds_add_u32 v0, v255
ds_add_u32 v77, v128 offset:257

// ---- VOP3: the funnel shift, a destination and three sources -----------------------------------
v_alignbit_b32 v0, v1, v2, v3
v_alignbit_b32 v255, v255, v255, v255
v_alignbit_b32 v37, s101, v40, s5

// ---- MUBUF: the cache invalidations, which name nothing -----------------------------------------
buffer_gl0_inv
buffer_gl1_inv
buffer_gl0_inv
buffer_gl1_inv
buffer_gl0_inv
buffer_gl1_inv

// ---- MUBUF: the other one-word accesses and atomics, laid out so a refusal names them ----------
buffer_load_ubyte v2, v40, s[8:11], s3 offen
buffer_load_ubyte v255, v255, s[100:103], s105 offen
buffer_load_ubyte v128, v0, s[64:67], s101 idxen
buffer_load_ubyte v17, v128, s[36:39], s0 offen glc
buffer_load_sbyte v2, v40, s[8:11], s3 offen
buffer_load_sbyte v255, v255, s[100:103], s105 offen
buffer_load_sbyte v128, v0, s[64:67], s101 idxen
buffer_load_sbyte v17, v128, s[36:39], s0 offen glc
buffer_load_sshort v2, v40, s[8:11], s3 offen
buffer_load_sshort v255, v255, s[100:103], s105 offen
buffer_load_sshort v128, v0, s[64:67], s101 idxen
buffer_load_sshort v17, v128, s[36:39], s0 offen glc
buffer_store_byte v2, v40, s[8:11], s3 offen
buffer_store_byte v255, v255, s[100:103], s105 offen
buffer_store_byte v128, v0, s[64:67], s101 idxen
buffer_store_byte v17, v128, s[36:39], s0 offen glc
buffer_store_short v2, v40, s[8:11], s3 offen
buffer_store_short v255, v255, s[100:103], s105 offen
buffer_store_short v128, v0, s[64:67], s101 idxen
buffer_store_short v17, v128, s[36:39], s0 offen glc
buffer_atomic_swap v2, v40, s[8:11], s3 offen
buffer_atomic_swap v255, v255, s[100:103], s105 offen
buffer_atomic_swap v128, v0, s[64:67], s101 idxen
buffer_atomic_swap v17, v128, s[36:39], s0 offen glc
buffer_atomic_sub v2, v40, s[8:11], s3 offen
buffer_atomic_sub v255, v255, s[100:103], s105 offen
buffer_atomic_sub v128, v0, s[64:67], s101 idxen
buffer_atomic_sub v17, v128, s[36:39], s0 offen glc
buffer_atomic_smin v2, v40, s[8:11], s3 offen
buffer_atomic_smin v255, v255, s[100:103], s105 offen
buffer_atomic_smin v128, v0, s[64:67], s101 idxen
buffer_atomic_smin v17, v128, s[36:39], s0 offen glc
buffer_atomic_umin v2, v40, s[8:11], s3 offen
buffer_atomic_umin v255, v255, s[100:103], s105 offen
buffer_atomic_umin v128, v0, s[64:67], s101 idxen
buffer_atomic_umin v17, v128, s[36:39], s0 offen glc
buffer_atomic_smax v2, v40, s[8:11], s3 offen
buffer_atomic_smax v255, v255, s[100:103], s105 offen
buffer_atomic_smax v128, v0, s[64:67], s101 idxen
buffer_atomic_smax v17, v128, s[36:39], s0 offen glc
buffer_atomic_umax v2, v40, s[8:11], s3 offen
buffer_atomic_umax v255, v255, s[100:103], s105 offen
buffer_atomic_umax v128, v0, s[64:67], s101 idxen
buffer_atomic_umax v17, v128, s[36:39], s0 offen glc
buffer_atomic_and v2, v40, s[8:11], s3 offen
buffer_atomic_and v255, v255, s[100:103], s105 offen
buffer_atomic_and v128, v0, s[64:67], s101 idxen
buffer_atomic_and v17, v128, s[36:39], s0 offen glc
buffer_atomic_xor v2, v40, s[8:11], s3 offen
buffer_atomic_xor v255, v255, s[100:103], s105 offen
buffer_atomic_xor v128, v0, s[64:67], s101 idxen
buffer_atomic_xor v17, v128, s[36:39], s0 offen glc
buffer_atomic_inc v2, v40, s[8:11], s3 offen
buffer_atomic_inc v255, v255, s[100:103], s105 offen
buffer_atomic_inc v128, v0, s[64:67], s101 idxen
buffer_atomic_inc v17, v128, s[36:39], s0 offen glc
buffer_atomic_dec v2, v40, s[8:11], s3 offen
buffer_atomic_dec v255, v255, s[100:103], s105 offen
buffer_atomic_dec v128, v0, s[64:67], s101 idxen
buffer_atomic_dec v17, v128, s[36:39], s0 offen glc
buffer_atomic_cmpswap v[2:3], v40, s[8:11], s3 offen
buffer_atomic_cmpswap v[254:255], v255, s[100:103], s105 offen
buffer_atomic_cmpswap v[128:129], v0, s[64:67], s101 idxen
buffer_atomic_cmpswap v[17:18], v128, s[36:39], s0 offen glc
buffer_atomic_add_x2 v[2:3], v40, s[8:11], s3 offen
buffer_atomic_add_x2 v[254:255], v255, s[100:103], s105 offen
buffer_atomic_add_x2 v[128:129], v0, s[64:67], s101 idxen
buffer_atomic_add_x2 v[17:18], v128, s[36:39], s0 offen glc

// ---- DS: the paired 64-bit write, an address, two value pairs and two offsets in 8-byte units --
ds_write2_b64 v1, v[2:3], v[4:5] offset0:4 offset1:9
ds_write2_b64 v200, v[8:9], v[254:255] offset0:1 offset1:255
ds_write2_b64 v0, v[130:131], v[76:77] offset0:255 offset1:2
ds_write2_b64 v77, v[0:1], v[2:3] offset0:128 offset1:64
ds_write2_b64 v255, v[254:255], v[200:201] offset0:12 offset1:200

// ---- DS: the paired 64-bit read, four destination words, an address and two offsets ----------
ds_read2_b64 v[4:7], v1 offset0:4 offset1:9
ds_read2_b64 v[200:203], v9 offset0:1 offset1:255
ds_read2_b64 v[0:3], v130 offset0:255 offset1:2
ds_read2_b64 v[252:255], v77 offset0:128 offset1:64
ds_read2_b64 v[77:80], v255 offset0:12 offset1:200

// ---- v_cmpx_lt_i16: VOPC, the execution mask from two sixteen-bit sources -------------
//
// Grand Theft Auto V's ray-traversal passes at 0x74005b857200 (0x518) and 0x74005b854a00
// (0x12a4) narrow the execution mask on a signed sixteen-bit compare. Varied inline, vector and
// scalar first sources, and vector second sources across the file.
v_cmpx_lt_i16_e32 0, v2
v_cmpx_lt_i16_e32 v200, v130
v_cmpx_lt_i16_e32 s101, v9
v_cmpx_lt_i16_e32 -1, v255
v_cmpx_lt_i16_e32 4, v77

// ---- v_lshlrev_b64: a 64-bit shift left by a distance --------------------------------
//
// Grand Theft Auto V's program at 0x74005b858800 (0xf4) scales a 64-bit index into a byte offset.
// Varied distances - vector, scalar and inline - and register pairs spread across the file.
v_lshlrev_b64 v[4:5], 0, s[4:5]
v_lshlrev_b64 v[100:101], v200, v[130:131]
v_lshlrev_b64 v[254:255], s30, v[12:13]
v_lshlrev_b64 v[8:9], 4, v[44:45]
v_lshlrev_b64 v[40:41], v77, s[70:71]

// ---- DS: a lane's own word of the local share, at the offset, m0 and four bytes a lane ----------
//
// Grand Theft Auto V's program at 0x74005b858800 keeps a word a lane in the local share with
// ds_write_addtid_b32 (0x358) and reads it back with ds_read_addtid_b32. One data or destination
// register and the offset: registers across the file, offsets at both ends.
ds_write_addtid_b32 v1 offset:2056
ds_write_addtid_b32 v255 offset:65535
ds_write_addtid_b32 v0
ds_write_addtid_b32 v128 offset:4
ds_write_addtid_b32 v77 offset:32768
ds_read_addtid_b32 v1 offset:2056
ds_read_addtid_b32 v255 offset:65535
ds_read_addtid_b32 v0
ds_read_addtid_b32 v128 offset:4
ds_read_addtid_b32 v77 offset:32768

// ---- v_med3_u32: VOP3, the median of three unsigned words ----------------------------
//
// Grand Theft Auto V's ray-traversal passes at 0x74005b857200 and 0x74005b854a00 clamp an index
// with it. Varied vector, scalar and inline sources in each of the three fields.
v_med3_u32 v3, v4, v5, v6
v_med3_u32 v200, s5, v9, v190
v_med3_u32 v255, v12, s30, 64
v_med3_u32 v9, -1, v77, v3
v_med3_u32 v130, v88, 4, s101

// ---- v_pk_add_i16 and v_pk_sub_i16: VOP3P, two sources --------------------------------------
//
// The same passes step a pair of sixteen-bit counters at once.
v_pk_add_i16 v0, v1, v2
v_pk_add_i16 v200, v5, v9
v_pk_add_i16 v255, v12, s30
v_pk_add_i16 v9, s101, v77
v_pk_add_i16 v130, v88, 4
v_pk_sub_i16 v0, v1, v2
v_pk_sub_i16 v200, v5, v9
v_pk_sub_i16 v255, v12, s30
v_pk_sub_i16 v9, s101, v77
v_pk_sub_i16 v130, v88, 4

// ---- v_cmp_eq_u64_e64 and v_cmp_ne_u64_e64: VOP3, a mask from two 64-bit sources -------------
//
// The program at 0x74005b854a00 compares 64-bit addresses into a scalar pair.
v_cmp_ne_u64_e64 s[2:3], v[4:5], v[6:7]
v_cmp_ne_u64_e64 s[100:101], s[4:5], v[190:191]
v_cmp_ne_u64_e64 s[40:41], v[254:255], -1
v_cmp_ne_u64_e64 vcc, 4, v[76:77]
v_cmp_ne_u64_e64 s[70:71], v[130:131], s[100:101]
v_cmp_eq_u64_e64 s[2:3], v[4:5], v[6:7]
v_cmp_eq_u64_e64 s[100:101], s[4:5], v[190:191]
v_cmp_eq_u64_e64 s[40:41], v[254:255], -1
v_cmp_eq_u64_e64 vcc, 4, v[76:77]
v_cmp_eq_u64_e64 s[70:71], v[130:131], s[100:101]

// ---- s_subb_u32: SOP2, a difference with the condition code's borrow ------------------------
//
// The programs at 0x74005b858800 and 0x74005b854a00 subtract 64-bit values a half at a time.
s_subb_u32 s0, s0, s1
s_subb_u32 s101, s5, s100
s_subb_u32 s7, -1, s13
s_subb_u32 s60, s9, 64
s_subb_u32 s9, vcc_lo, s2

// ---- s_bitreplicate_b64_b32: SOP1, each bit of a word twice into a pair -----------------------
//
// The program at 0x74005b858800 builds a 64-bit mask from a literal with it.
s_bitreplicate_b64_b32 s[0:1], s2
s_bitreplicate_b64_b32 s[100:101], s101
s_bitreplicate_b64_b32 s[48:49], 64
s_bitreplicate_b64_b32 s[22:23], -1
s_bitreplicate_b64_b32 vcc, s13
