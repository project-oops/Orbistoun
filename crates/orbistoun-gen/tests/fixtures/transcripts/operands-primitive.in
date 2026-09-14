// The GL cube's vertex program: three SOPP immediates and two VOP2 opcodes no other probe
// reaches (worklog 545 §2). Solved here so the translator can read what they operate on
// once it can name them; the fixture `../primitive.s` is where the names come from.

// ---- SOPP with a sixteen-bit immediate and nothing else ------------------------------
//
// Nop, prefetch hint and send-message each carry one immediate in the low half of the
// word. The assembler accepts the full sixteen bits for all three (measured: `s_nop 0xffff`
// and `s_inst_prefetch 0xffff` both assemble), so the samples reach the top of the field.
// A probe that stayed at the small values the shader uses would leave a two- and a
// three-bit window explaining every sample as well as the real one, and the solver refuses
// an ambiguous width rather than picking.
s_nop 0
s_nop 7
s_nop 0x3fff
s_nop 0xffff
s_inst_prefetch 0x1
s_inst_prefetch 0x3
s_inst_prefetch 0x1f
s_inst_prefetch 0xffff
// Send-message values the reference prints as numbers. A message it knows prints by name -
// `sendmsg(MSG_GS_ALLOC_REQ)` - which is text no field explains, so the probes keep clear of
// the ids it knows. The field they pin is the same sixteen bits the shader's
// `MSG_GS_ALLOC_REQ` (9) sits in; reading the number as a message is the translator's
// job, as a branch offset's sign is.
s_sendmsg 0x1234
s_sendmsg 0x4321
s_sendmsg 0x7f0f
s_sendmsg 0xf0f0
s_sendmsg 0xffff

// ---- v_lshrrev_b32: VOP2, a shift count in the shared source field ------------------
//
// Each source gets its own high sample, and a special register and inline constants in
// the first source pin its nine-bit width, as the other VOP2 probes do.
v_lshrrev_b32_e32 v20, 2, v15
v_lshrrev_b32_e32 v255, s101, v200
v_lshrrev_b32_e32 v7, -1, v130
v_lshrrev_b32_e32 v130, v240, v66
v_lshrrev_b32_e32 v3, vcc_lo, v250
v_lshrrev_b32_e32 v99, 0, v9

// ---- v_add_co_ci_u32: VOP2 add with a carry in and a carry out, both implicit --------
//
// The carry mask is `vcc` on both sides and nothing else is legal there in this encoding
// (the assembler refuses an SGPR pair; that is the three-operand form's business), so both
// slots solve as implicit - the shape the compares' implicit destination already has. The
// three fields that are encoded are the ordinary VOP2 ones.
v_add_co_ci_u32_e32 v19, vcc, s3, v1, vcc
v_add_co_ci_u32_e32 v250, vcc, -1, v240, vcc
v_add_co_ci_u32_e32 v3, vcc, v200, v100, vcc
v_add_co_ci_u32_e32 v77, vcc, s100, v9, vcc
v_add_co_ci_u32_e32 v130, vcc, 0, v255, vcc
v_add_co_ci_u32_e32 v1, vcc, vcc_lo, v66, vcc
