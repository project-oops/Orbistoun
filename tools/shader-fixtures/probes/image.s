// MIMG operand layouts. The family had none for any opcode, so every image instruction
// decoded with no operands at all - the shape that passes a differential test vacuously,
// and the shape the GL cube's textured pixel shader stopped at: `image_sample_lz ... has no
// operand layout; cannot translate what it operates on` (orbistoun worklog 545).
//
// target triple: amdgcn-mesa-mesa3d
//
// # What each operand is, and what the solver has to separate
//
// Printed order is data, address, resource, sampler, dmask. The data and address operands
// are vector registers; the resource is **eight** consecutive scalar registers and the
// sampler **four**, and each is named in the encoding by a five-bit field holding a quarter
// of its base register - the same quarter-scale reading the buffer descriptors already use.
// So the samples put resources high enough that a four-bit field cannot explain them:
// `s[80:87]` is 20 and `s[92:99]` is 23, both past the fifteen a four-bit window reaches.
//
// `dim:SQ_RSRC_IMG_2D` is a modifier printed as a symbolic name, like MTBUF's
// `format:[...]`, and is skipped for the same reason: the field exists, its spelling is not
// a number, and inventing a code for it is what this repository refuses to do.
//
// # Why the register counts vary with dmask
//
// `dmask` selects which channels come back, and the assembler refuses a data operand whose
// register count does not match its population count. So the sizes move together here; that
// is the instruction's rule, not a probe convention.

// ---- image_sample_lz: each field high in turn --------------------------------------
image_sample_lz v[4:7], v[2:3], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample_lz v[100:103], v[200:201], s[80:87], s[92:95] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample_lz v250, v[10:11], s[40:47], s[60:63] dmask:0x1 dim:SQ_RSRC_IMG_2D
image_sample_lz v[8:9], v[254:255], s[12:19], s[20:23] dmask:0x3 dim:SQ_RSRC_IMG_2D
image_sample_lz v[240:243], v[100:101], s[60:67], s[96:99] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample_lz v[16:18], v[44:45], s[92:99], s[40:43] dmask:0x7 dim:SQ_RSRC_IMG_2D
// With `unorm` set, which is the bit immediately above `dmask`. Without these the field
// solved one bit too wide - correct on every sample here and wrong by sixteen on any shader
// that samples with unnormalised coordinates.
image_sample_lz v[4:7], v[2:3], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D unorm
image_sample_lz v[60:61], v[80:81], s[64:71], s[76:79] dmask:0x3 dim:SQ_RSRC_IMG_2D unorm

// ---- image_sample ------------------------------------------------------------------
image_sample v[4:7], v[2:3], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample v[60:61], v[30:31], s[64:71], s[76:79] dmask:0x3 dim:SQ_RSRC_IMG_2D
image_sample v128, v[190:191], s[80:87], s[92:95] dmask:0x1 dim:SQ_RSRC_IMG_2D
image_sample v[200:203], v[8:9], s[20:27], s[4:7] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample v[33:35], v[250:251], s[92:99], s[60:63] dmask:0x7 dim:SQ_RSRC_IMG_2D
image_sample v[4:7], v[2:3], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D unorm
image_sample v90, v[120:121], s[40:47], s[20:23] dmask:0x1 dim:SQ_RSRC_IMG_2D unorm

// ---- image_sample_l: three address registers, so the address field is not the size --
image_sample_l v[4:7], v[2:4], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample_l v[70:73], v[100:102], s[80:87], s[92:95] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample_l v220, v[30:32], s[40:47], s[20:23] dmask:0x1 dim:SQ_RSRC_IMG_2D
image_sample_l v[12:13], v[240:242], s[92:99], s[76:79] dmask:0x3 dim:SQ_RSRC_IMG_2D
image_sample_l v[4:7], v[2:4], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D unorm
image_sample_l v66, v[88:90], s[60:67], s[92:95] dmask:0x1 dim:SQ_RSRC_IMG_2D unorm

// ---- image_load and image_store: four operands, no sampler --------------------------
image_load v[8:11], v[12:13], s[16:23] dmask:0xf dim:SQ_RSRC_IMG_2D
image_load v200, v[250:251], s[80:87] dmask:0x1 dim:SQ_RSRC_IMG_2D
image_load v[100:101], v[2:3], s[40:47] dmask:0x3 dim:SQ_RSRC_IMG_2D
image_load v[30:32], v[190:191], s[92:99] dmask:0x7 dim:SQ_RSRC_IMG_2D
image_load v[8:11], v[12:13], s[16:23] dmask:0xf dim:SQ_RSRC_IMG_2D unorm
image_load v44, v[60:61], s[80:87] dmask:0x1 dim:SQ_RSRC_IMG_2D unorm
image_store v[24:27], v[28:29], s[64:71] dmask:0xf dim:SQ_RSRC_IMG_2D
image_store v255, v[100:101], s[92:99] dmask:0x1 dim:SQ_RSRC_IMG_2D
image_store v[40:41], v[200:201], s[20:27] dmask:0x3 dim:SQ_RSRC_IMG_2D
image_store v[60:62], v[10:11], s[80:87] dmask:0x7 dim:SQ_RSRC_IMG_2D
image_store v[24:27], v[28:29], s[64:71] dmask:0xf dim:SQ_RSRC_IMG_2D unorm
image_store v70, v[140:141], s[20:27] dmask:0x1 dim:SQ_RSRC_IMG_2D unorm
