// Scalar families whose layout is already established, included as a control: the
// solver should recover the fields already declared, and disagreeing with them would
// mean the solver is wrong rather than the table.
s_add_i32 s5, s6, s7
s_add_i32 s15, s16, s17
s_mov_b32 s3, s4
s_mov_b32 s13, s14
s_movk_i32 s5, 0x1234
s_movk_i32 s9, 0x5678
s_movk_i32 s19, 0x9abc

s_add_i32 s100, s99, s98
s_mov_b32 s101, s100
s_movk_i32 s100, 0x4321

// Codes above the register range. Scalar registers stop at 101, so samples using only
// registers can be explained by a seven-bit field where the real one is eight - and the
// solver picked exactly that, correct on every sample and wrong on the first literal.
// Inline constants and special registers reach the top of the space and pin the width.
s_mov_b32 s5, 0
s_mov_b32 s7, 64
s_mov_b32 s9, vcc_lo
s_mov_b32 exec_lo, s11
s_add_i32 s5, 0, s6
s_add_i32 s7, vcc_lo, s8

s_add_i32 s9, s10, 0
s_add_i32 s11, 1.0, s12

// ---- s_mov_b64 ------------------------------------------------------------------
//
// A register pair on both sides. Both fields hold the *first* register of the pair
// rather than the pair number, which is the thing worth pinning: aligned operands
// always admit a scaled reading of half the value, and the two disagree the moment a
// pair starts at an odd boundary the assembler will not emit. Spreading the samples
// across the whole file separates them by value instead.
s_mov_b64 s[2:3], s[4:5]
s_mov_b64 s[10:11], s[20:21]
s_mov_b64 s[30:31], s[8:9]
s_mov_b64 s[64:65], s[70:71]
s_mov_b64 s[96:97], s[100:101]
s_mov_b64 s[100:101], s[36:37]
s_mov_b64 s[44:45], s[96:97]
// Specials and inline constants, which reach past the register range and pin the
// source field's width - the same trap `s_mov_b32` fell into above.
s_mov_b64 s[6:7], exec
s_mov_b64 exec, s[12:13]
s_mov_b64 s[14:15], 0
s_mov_b64 s[16:17], -1

// ---- 64-bit scalar logic, which is how a mask is computed ------------------------
//
// A guest narrows the execution mask by anding it with a comparison result and widens
// it by oring; `s_andn2_b64` is how an else-branch takes the lanes the if-branch did
// not. All three name register pairs by their low half, same as `s_mov_b64`, and the
// execution mask itself is an ordinary operand to them - which is the whole reason the
// mask is a *value* in the wavefront model rather than something the model manages.
s_and_b64 s[0:1], s[2:3], s[4:5]
s_and_b64 s[10:11], s[30:31], s[60:61]
s_and_b64 exec, exec, s[8:9]
s_and_b64 s[96:97], s[100:101], exec
s_or_b64 s[2:3], s[4:5], s[6:7]
s_or_b64 s[70:71], s[12:13], s[90:91]
s_or_b64 exec, s[20:21], exec
s_or_b64 s[40:41], exec, s[64:65]
s_andn2_b64 s[0:1], s[2:3], s[4:5]
s_andn2_b64 exec, exec, s[16:17]
s_andn2_b64 s[86:87], s[100:101], s[34:35]
s_andn2_b64 s[24:25], exec, s[96:97]
// Inline constants, which reach past the register range and pin the source widths.
s_and_b64 s[6:7], s[8:9], -1
s_or_b64 s[14:15], 0, s[18:19]
s_andn2_b64 s[22:23], -1, s[26:27]

// Every one of the three above solved with one source field a bit too narrow, and a
// different one each time - whichever slot happened never to receive a value above 127.
// They disagreed with each other and with `s_add_i32`, which shares the encoding.
//
// The rule this keeps failing to apply: **each field needs its own high sample.** A
// high value somewhere in the instruction does nothing for the field that did not get
// one. So each opcode below takes an inline constant in the first source and then in
// the second.
s_and_b64 s[0:1], -1, s[4:5]
s_and_b64 s[2:3], s[6:7], 0
s_or_b64 s[4:5], -1, s[8:9]
s_or_b64 s[6:7], s[10:11], 0
s_andn2_b64 s[8:9], 0, s[12:13]
s_andn2_b64 s[10:11], s[14:15], -1

// ---- SOPP branches: a signed word offset in the low half -------------------------
//
// The family declares an empty operand layout, which is a genuine claim about most of
// it - `s_endpgm` and `s_waitcnt` carry no register operand. Branches carry a target,
// and without a per-opcode layout it is not decoded at all, so a translator cannot know
// where a branch goes.
//
// The offset is *signed* and the reference prints it unsigned: -6 appears as 65530.
// That stays as-is here. The decoder reports the field as encoded, so it keeps agreeing
// with the reference operand for operand; reading it as signed is the translator's job,
// because the width and signedness are properties of the instruction rather than of the
// bits. The same split the operand table already makes for 64-bit operand names.
s_branch 3
s_branch 65530
s_branch 32767
s_cbranch_scc0 4
s_cbranch_scc0 60000
s_cbranch_scc1 3
s_cbranch_scc1 65535
s_cbranch_vccz 6
s_cbranch_vccz 40000
s_cbranch_vccnz 7
s_cbranch_vccnz 65000
s_cbranch_execz 8
s_cbranch_execz 33000
s_cbranch_execnz 65530
s_cbranch_execnz 12345

// ---- SOPC: the scalar compares, which are what set the condition code -------------
//
// No destination operand at all - the result is a single bit of hidden state the
// branches read. That makes them the mirror image of the vector compares, which have an
// implicit sixty-four-bit destination: same shape of problem, and the solver has to be
// told nothing special for these because there is simply no third operand to find.
//
// Each source gets its own high sample, and inline constants reach past the register
// range to pin the widths.
s_cmp_eq_i32 s4, s5
s_cmp_eq_i32 s100, s101
s_cmp_eq_i32 -1, s9
s_cmp_eq_i32 s12, 0
s_cmp_lg_i32 s70, 0
s_cmp_lg_i32 s3, s100
s_cmp_lg_i32 0, s44
s_cmp_gt_i32 s100, -1
s_cmp_gt_i32 s6, s101
s_cmp_gt_i32 -1, s20
s_cmp_ge_i32 s9, s101
s_cmp_ge_i32 s101, s9
s_cmp_ge_i32 0, s30
s_cmp_lt_i32 s2, 1
s_cmp_lt_i32 s101, s70
s_cmp_lt_i32 -1, s40
s_cmp_le_i32 s3, s4
s_cmp_le_i32 s100, s101
s_cmp_le_i32 0, s50

// The agreement check found these: `s_cmp_ge_i32` and `s_cmp_le_i32` solved their second
// source at seven bits where their four siblings got eight, because no sample put an
// inline constant in that slot. Found by the tool rather than by reading the rows, which
// is the first time - it is the fifth occurrence of this exact fault.
s_cmp_ge_i32 s5, -1
s_cmp_ge_i32 s60, 0
s_cmp_le_i32 s6, -1
s_cmp_le_i32 s61, 0

// ---- SOPK and the 32-bit scalar arithmetic --------------------------------------
//
// SOPK carries a sixteen-bit immediate and a destination and nothing else. The
// immediate is signed for the arithmetic forms and the reference prints it unsigned,
// same as a branch offset - so the decoder reports the field as encoded and reading it
// as signed is the translator's job.
s_movk_i32 s0, 0x1234
s_movk_i32 s101, 0xffff
s_movk_i32 s45, 0x7fff
s_cmpk_eq_i32 s3, 0x40
s_cmpk_eq_i32 s100, 0xffff
s_cmpk_lg_i32 s70, 0x8000
s_cmpk_lg_i32 s9, 0x1
s_addk_i32 s12, 0x10
s_addk_i32 s101, 0xfffe
s_mulk_i32 s90, 0x200
s_mulk_i32 s7, 0xffff

s_add_i32 s0, s1, s2
s_add_i32 s100, s101, -1
s_add_i32 s5, 0, s6
s_sub_i32 s1, s2, s3
s_sub_i32 s101, -1, s100
s_sub_i32 s7, s8, 0
s_and_b32 s2, s3, s4
s_and_b32 s100, -1, s101
s_and_b32 s9, s10, 0
s_or_b32 s3, s4, s5
s_or_b32 s101, s100, -1
s_or_b32 s11, 0, s12
s_xor_b32 s4, s5, s6
s_xor_b32 s100, -1, s101
s_xor_b32 s13, s14, 0

// ---- s_wqm_b64: whole quad mode --------------------------------------------------
//
// Sets each group of four bits of the result if any of the corresponding four bits of
// the source is set. A fragment shader uses it so that a derivative computed across a
// quad has all four pixels live even where only one is covered.
s_wqm_b64 s[0:1], s[2:3]
s_wqm_b64 s[100:101], s[70:71]
s_wqm_b64 exec, s[8:9]
s_wqm_b64 s[34:35], exec
s_wqm_b64 s[6:7], -1
s_wqm_b64 s[10:11], 0

// The two that carry no register operand at all. An empty operand list is a different
// claim from an absent one, and the solver records the name either way - which is what
// dispatching on names needs.
s_endpgm
s_waitcnt lgkmcnt(0)
s_waitcnt vmcnt(0)
s_waitcnt expcnt(0)
