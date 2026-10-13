// The scalar unsigned orderings, which no compiled fixture reaches.
//
// A GL shader's loop bound and integer divide compile to them: gl2-probe's checks after
// shadow-compare stop at the first, SOPC opcode 9. The compiled fixtures only ever produced the
// signed orderings and the two unsigned equalities, which sign does not change, so the four
// orderings whose answer does depend on sign had no recorded name.
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names. Each with a literal and with a register second source, high registers among them.
s_cmp_gt_u32 s0, s1
s_cmp_gt_u32 s45, 0x3f
s_cmp_ge_u32 s2, s3
s_cmp_ge_u32 s101, 0x12345678
s_cmp_lt_u32 s4, s5
s_cmp_lt_u32 s70, 7
s_cmp_le_u32 s6, s7
s_cmp_le_u32 s90, 0xffffffff
s_endpgm
