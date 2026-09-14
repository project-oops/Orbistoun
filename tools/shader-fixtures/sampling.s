// Texture sampling beyond the one form a compiled fixture reaches.
//
// `image.ll` compiles to exactly one MIMG instruction, `image_sample` with `dmask:0x1`,
// because that is what the one intrinsic anybody can write from IR produces. Every other
// opcode in the family was unnamed - including `image_sample_lz`, which is what a real
// pixel shader uses when it has no derivatives to compute a level of detail from, and which
// is the instruction the GL cube's textured shader runs on the console (orbistoun worklogs
// 545 and 548). An unnamed opcode has no translation, because dispatch is by name.
//
// Written by hand and assembled, like `unreached.s` and `primitive.s`, with the same
// weakness: these are instructions somebody thought of rather than ones a compiler reached
// for. The reference still decides the bytes and the boundaries.
//
// `dmask` selects which channels come back and the data register count must match its
// population count - the assembler refuses `dmask:0x7` with four registers - so the sizes
// here vary together deliberately.

// The graphics environment. The compute one refuses a sampling shader outright, and does it
// by crashing rather than diagnosing, which cost a run to find (see `image.ll`).
// target triple: amdgcn-mesa-mesa3d

// ---- image_sample_lz: sample at level zero, no derivatives -------------------------
image_sample_lz v[4:7], v[2:3], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample_lz v[100:103], v[200:201], s[80:87], s[92:95] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample_lz v250, v[10:11], s[40:47], s[60:63] dmask:0x1 dim:SQ_RSRC_IMG_2D
image_sample_lz v[8:9], v[254:255], s[12:19], s[20:23] dmask:0x3 dim:SQ_RSRC_IMG_2D

// ---- image_sample, the compiled fixture's opcode, at other sizes --------------------
image_sample v[4:7], v[2:3], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample v[60:61], v[30:31], s[64:71], s[76:79] dmask:0x3 dim:SQ_RSRC_IMG_2D

// ---- image_sample_l: the level comes from a third address register ------------------
image_sample_l v[4:7], v[2:4], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D

// ---- Load and store, which carry no sampler at all ----------------------------------
image_load v[8:11], v[12:13], s[16:23] dmask:0xf dim:SQ_RSRC_IMG_2D
image_load v200, v[250:251], s[80:87] dmask:0x1 dim:SQ_RSRC_IMG_2D
image_store v[24:27], v[28:29], s[64:71] dmask:0xf dim:SQ_RSRC_IMG_2D

// A trailing scalar instruction, so the last MIMG's length is asserted by something
// following it rather than by the end of the stream.
s_endpgm
