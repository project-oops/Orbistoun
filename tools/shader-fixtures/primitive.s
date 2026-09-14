// The five opcodes the GL cube's vertex program runs that no other fixture reaches.
//
// The program is oops-sdk's hand-assembled NGG primitive shader, the one the console ran
// for oracle records A and B (orbistoun worklogs 539 and 545). Its first word is an
// instruction-prefetch hint; it sizes its geometry-engine allocation with a send-message
// after a nop wait state; and it forms a 64-bit vertex-buffer address with a shift and a
// carry-in add. No compiled fixture emits any of the five, so the translator could not
// even name them: the submission pipeline stopped at byte 0 of the shader with "no
// recorded name for that opcode" (worklog 545 §2).
//
// Written by hand and assembled, like `unreached.s`: the reference decides the bytes and
// the boundaries, so a wrong length or opcode field fails here exactly as it would for a
// compiled fixture. The words are the ones oops-sdk wrote from the published instruction
// set - the prefetch here is byte for byte the shader's first word, 0xbfa00001 - and
// nothing in this file came from any other implementation.
//
// The send-message spelling is the reference's own: it prints a message it knows by name
// and any other value as a number. `MSG_GS_ALLOC_REQ` assembles to 9, which is the value in
// the console-run shader (0xbf900009); the differential test carries that measured code the
// way it carries the export targets.

// ---- The prologue, in the vertex program's order -----------------------------------
s_inst_prefetch 0x1
s_mov_b32 s12, exec_lo
s_mov_b32 m0, 0x1003
s_nop 0
s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
s_mov_b32 exec_lo, 1

// ---- Address arithmetic: lane * 16, then >> 2, then a 64-bit add with carry --------
v_lshlrev_b32_e32 v15, 4, v14
v_lshrrev_b32_e32 v20, 2, v15
v_add_co_ci_u32_e32 v19, vcc, s3, v1, vcc

// ---- The store both of the cube's shaders publish their canary with ----------------
//
// A global store whose address is the VGPR pair, with **no scalar base**. Here because the
// translator had the code for that wrong: it expected the top of the field, 0x7f, and the
// reference emits 0x7d - which our own operand table names `null`, and which is the code the
// console's shaders carry. The two agreed with each other and with nothing else, because no
// committed fixture had ever contained the form (worklog 552). Now one does, so the next
// disagreement is a test failure rather than a translation that refuses a real shader.
global_store_dword v[8:9], v10, off offset:4
global_store_dword v[18:19], v1, off offset:0
// And the same instruction *with* a base, which encodes the register rather than the marker.
global_load_dword v3, v4, s[6:7]

// ---- More of each, so every length is asserted by what follows it ------------------
s_inst_prefetch 0x3
s_nop 7
s_nop 0x3fff
v_lshrrev_b32_e32 v255, s101, v200
v_lshrrev_b32_e32 v7, -1, v130
v_add_co_ci_u32_e32 v250, vcc, -1, v240, vcc
v_add_co_ci_u32_e32 v3, vcc, v200, v100, vcc
s_endpgm
