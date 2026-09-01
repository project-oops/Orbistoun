// The three encoding families nothing else in the fixture set produces.
//
// SOPK, MTBUF and VINTRP were transcribed from the published specification and never
// checked against a reference, because no LLVM IR the generator can write makes the
// compiler emit them: SOPK arises from specific constant patterns, MTBUF from typed
// buffer formats, and VINTRP from fragment interpolation. Every other family in the
// table had been verified differentially; these three were a footnote saying so (D085).
//
// Written by hand and assembled rather than compiled, which is a weaker fixture: the
// instruction chosen is one somebody thought of rather than one a compiler reached for,
// so this shows the table's rows are right about *these* instructions and not that they
// cover everything the family can express.
//
// It still settles the question that mattered. The reference decides the bytes and the
// boundaries, so a wrong mask, value, opcode field or instruction length fails here
// exactly as it would for a compiled fixture - and a wrong length is the catastrophic
// one, because it shifts every instruction after it.

// ---- SOPK: a scalar operation with a sixteen-bit immediate ---------------------
//
// Four bytes. The immediate is the whole low half, so a length error here shows up
// immediately as the next instruction landing mid-word.
s_movk_i32 s0, 0x1234
s_movk_i32 s45, 0xabcd
s_movk_i32 s101, 0x7fff
s_cmpk_eq_i32 s3, 0x40
s_cmpk_lg_i32 s70, 0xffff
s_addk_i32 s12, 0x10
s_mulk_i32 s90, 0x200

// ---- VINTRP: fragment interpolation --------------------------------------------
//
// Four bytes. The attribute and channel sit in the same word as the opcode, and the
// opcode field is only two bits wide - narrow enough that getting its position wrong
// still decodes as *some* instruction in the family, which is why a reference is worth
// more here than reading the row again.
v_interp_p1_f32 v0, v1, attr0.x
v_interp_p2_f32 v2, v3, attr0.x
v_interp_mov_f32 v4, p0, attr0.x
v_interp_p1_f32 v5, v6, attr7.y
v_interp_p2_f32 v7, v8, attr12.z
v_interp_mov_f32 v9, p10, attr31.w

// ---- MTBUF: typed buffer access -------------------------------------------------
//
// Eight bytes, and the only one of the three that is not four - so if the table's width
// for this family were wrong, every instruction after the first would be misaligned and
// the offsets would diverge and stay diverged. That is precisely what the differential
// test asserts on.
//
// The format goes *before* the addressing modifier, which is the order the reference
// prints - written the other way round llvm-mc rejects it outright rather than
// accepting it and meaning something else, which is the failure to want.
//
// The format names are this generation's. It unified the previous generation's separate
// data and number formats into one `BUF_FMT_*` enumeration, so `BUF_DATA_FORMAT_32`
// is not merely renamed - there is no longer a field for it to name.
tbuffer_load_format_x v0, v1, s[8:11], 0 format:[BUF_FMT_32_UINT] idxen
tbuffer_load_format_xy v[2:3], v4, s[12:15], 0 format:[BUF_FMT_32_32_FLOAT] idxen
tbuffer_store_format_x v5, v6, s[16:19], 0 format:[BUF_FMT_32_FLOAT] idxen
tbuffer_load_format_xyzw v[8:11], v12, s[20:23], 0 format:[BUF_FMT_32_32_32_32_UINT] idxen
tbuffer_load_format_x v20, v21, s[24:27], s5 format:[BUF_FMT_8_UINT] offen
tbuffer_store_format_xyzw v[24:27], v28, s[28:31], s7 format:[BUF_FMT_32_32_32_32_FLOAT] idxen

// A trailing scalar instruction, so the last MTBUF's length is asserted by something
// following it rather than by the end of the stream.
s_endpgm
