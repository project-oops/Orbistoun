// A scalar bit set, a signed scalar maximum and a vector leading-zero count, which no compiled fixture
// reaches.
//
// Grand Theft Auto V's compute program at 0x400003ce2a00 stops at the first of them: SOP1 opcode
// 0x1d (`0xbe911d94`), SOP2 opcode 0x8 and VOP1 opcode 0x39 had no recorded name; its program at
// byte 0x16680 at VOPC opcode 0x8, the unordered compare; its program at 0x400003ce6000 at VOP3
// opcodes 0x149 and 0x14f, a signed field extract and a byte align; its program at 0x400003e16400
// at SOP1 0x28, VOP3 0x378, VOP1 0x42 and SOPC 0x13.
//
// Written by hand and assembled, as `scalar-compare.s` is: the reference still decides the bytes
// and the names. GTA V's own word first, then register and literal sources.
s_bitset1_b32 s17, 20
s_bitset1_b32 s0, s1
s_max_i32 s0, s1, s2
s_max_i32 s45, s3, -5
v_ffbh_u32_e32 v0, v1
v_ffbh_u32_e32 v7, s9
v_cmp_u_f32_e32 vcc, v1, v1
v_cmp_u_f32_e32 vcc, s3, v7
v_bfe_i32 v0, v1, 2, v3
v_bfe_i32 v7, s9, v2, 16
v_alignbyte_b32 v6, 0, 0x3024240c, v5
v_alignbyte_b32 v1, v2, v3, 2
s_orn2_saveexec_b64 s[4:5], s[6:7]
s_orn2_saveexec_b64 s[10:11], vcc
v_permlanex16_b32 v1, v2, s3, s4
v_permlanex16_b32 v5, v6, 0, s9
v_movreld_b32_e32 v1, v2
v_movreld_b32_e32 v9, s3
s_cmp_lg_u64 s[0:1], s[2:3]
s_cmp_lg_u64 vcc, 0
s_endpgm
