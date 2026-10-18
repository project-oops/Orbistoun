// target triple: amdgcn-mesa-mesa3d
//
// A sample with explicit gradients, which no compiled fixture reaches: its two derivative pairs
// ahead of the coordinates. Grand Theft Auto V lists its address registers (NSA); the listed form's
// bracketed operand is one the differential reading does not split, and the translator reads the
// list from the encoding as for any other image access.
//
// Grand Theft Auto V's scene pixel shaders sample so (MIMG opcode 34 had no operand layout).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names.
image_sample_d v[4:7], v[0:5], s[4:11], s[12:15] dmask:0xf dim:SQ_RSRC_IMG_2D
image_sample_d v17, v[15:20], s[8:15], s[32:35] dmask:0x1 dim:SQ_RSRC_IMG_2D
s_endpgm
