// The opcodes the GL context's textured pixel shader runs that no other fixture reaches.
//
// The program is oops-sdk's hand-assembled textured pixel shader - its sampling prologue, the
// texture-environment combine, fog, the alpha test and the stipple/coverage slots
// (oops-sdk tools/shader/tex-prolog.s, tex-env.s, fog.s, alpha-test.s, polygon-stipple.s,
// coverage-tex.s). Neverball draws through it and the cube never does, so its first frame's
// submission stopped at byte 0x84 of the shader with "no recorded name for that opcode": the
// `s_wqm_b32` that puts the sample into whole-quad mode (orbistoun worklog 819).
//
// Written by hand and assembled, like `primitive.s`: the reference decides the bytes and the
// boundaries, so a wrong length or opcode field fails here exactly as it would for a compiled
// fixture. The instructions are the ones oops-sdk wrote from the published instruction set; the
// SDK assembles them for wave32, and the compares are spelled here with the implicit `vcc` this
// generator's wave64 target names - a VOPC's encoding carries no destination field, so the word
// is the same either way.

// ---- Whole-quad mode and the live mask (tex-prolog.s) ---------------------------------
s_wqm_b32 exec_lo, exec_lo
s_and_b32 exec_lo, exec_lo, vcc_lo

// ---- The branch over an unused slot (the sample, second-unit and combine slots) --------
// A word count, not a label: the reference prints a label by its name, which no decoder can
// compare with an offset - and the SDK's own `gl_ps_s_branch` writes the count directly.
s_branch 1
s_nop 0

// ---- The alpha test's compares (alpha-test.s) ------------------------------------------
v_cmp_eq_f32 vcc, v7, v12
v_cmp_ge_f32 vcc, v7, v12
v_cmp_gt_f32 vcc, v7, v12
v_cmp_le_f32 vcc, v7, v12
v_cmp_neq_f32 vcc, v7, v12

// ---- The polygon stipple's pixel address (polygon-stipple.s) ---------------------------
v_cvt_u32_f32_e32 v2, v2
v_and_b32_e32 v3, 31, v3
v_cmp_ne_u32_e32 vcc, 0, v3

// ---- Fog and the combine's subtractions (fog.s, tex-env.s) -----------------------------
v_sub_f32 v14, 1.0, v13
v_sub_f32_e32 v12, v14, v12

// ---- The export's half-float pack (the export slot, gl_ps_patch_export) ----------------
// Four colour floats into two registers of two halves each, for the compressed export an
// 8_8_8_8 target needs on this part (worklog 820).
v_cvt_pkrtz_f16_f32 v4, v4, v5
v_cvt_pkrtz_f16_f32 v5, v6, v7

s_endpgm
