// Memory families. A load writes a destination where a store reads data, from
// different bits - which is exactly why a per-family layout could not describe these.
s_load_dword s5, s[6:7], 0x10
s_load_dword s9, s[10:11], 0x24
s_load_dword s21, s[22:23], 0x1c8
s_load_dwordx2 s[2:3], s[8:9], 0x40
s_load_dwordx2 s[12:13], s[18:19], 0x88
ds_read_b32 v1, v2 offset:4
ds_read_b32 v9, v10 offset:8
ds_read_b32 v19, v20 offset:12
ds_write_b32 v3, v4 offset:4
ds_write_b32 v11, v12 offset:8
ds_write_b32 v21, v22 offset:12

// Same reason: high registers force the solver to a wide enough field.
s_load_dword s100, s[98:99], 0x3fc
ds_read_b32 v200, v201 offset:16
ds_write_b32 v202, v203 offset:20

// Same reasoning: spread and disorder rather than runs.
ds_read_b32 v77, v210 offset:24
ds_read_b32 v13, v190 offset:28
ds_write_b32 v240, v31 offset:36
ds_write_b32 v6, v155 offset:40
s_load_dword s33, s[70:71], 0x94
s_load_dwordx2 s[60:61], s[12:13], 0x2c

// Memory instructions the worklist ranks highest. Varied registers and offsets, and a
// spread of widths, so the solver can tell a load's destination field from a store's
// data field - which is the distinction a per-family layout could not express (D096).
s_load_dwordx4 s[8:11], s[20:21], 0x30
s_load_dwordx4 s[40:43], s[60:61], 0x140
s_load_dwordx4 s[16:19], s[4:5], 0x8
s_load_dwordx8 s[24:31], s[36:37], 0x60
s_load_dwordx8 s[48:55], s[10:11], 0xc4

// High and widely spread, to break a tie the solver reported on the store: with only
// low registers a nine-bit reading in the shared numbering fits alongside the plain
// eight-bit index, and the two decode differently.

// With a real scalar base rather than `off`.
//
// The tie the solver reported was genuine: `off` encodes the no-base case as all ones,
// so the bit above the data field was set in every sample, and a nine-bit window read
// exactly data-plus-256 - indistinguishable from the shared numbering. Varying that
// bit is the only thing that separates them.

// ---- flat memory --------------------------------------------------------------
//
// Always with a scalar base, never `off`. The no-base form does not print the base as
// an operand, so solving from it produces a layout with no field for it - and a
// translator using that layout would ignore a base address instead of refusing one.
//
// Addresses reach the top of the register file on purpose: with none above 110 a
// seven-bit field explains every sample of an eight-bit one, which is how the load
// solved one bit too narrow the first time.
global_store_dword v3, v9, s[10:11]
global_store_dword v77, v200, s[4:5]
global_store_dword v130, v44, s[20:21]
global_store_dword v255, v128, s[6:7]
global_store_dword v200, v255, s[30:31]
global_store_dword v11, v190, s[2:3]
global_load_dword v11, v22, s[8:9]
global_load_dword v190, v7, s[30:31]
global_load_dword v5, v200, s[12:13]
global_load_dword v255, v130, s[40:41]
global_load_dword v44, v255, s[16:17]
global_load_dword v128, v77, s[24:25]

// High scalar bases. Every base above was under sixty-four, so a six-bit field explains
// them all - and it also leaves the scale undetermined, since a scaled reading of half
// the value fits equally well. A base of one hundred separates both questions at once.
global_store_dword v3, v9, s[100:101]
global_store_dword v20, v30, s[70:71]
global_load_dword v40, v50, s[100:101]
global_load_dword v60, v70, s[70:71]

// ---- wide scalar loads, properly spread ---------------------------------------
//
// The first pass solved `s_load_dwordx2` with a six-bit destination and an eight-bit
// offset, and `s_load_dwordx4` with six and nine - against `s_load_dword`'s seven and
// sixteen, from the same encoding. Three samples each, none of them high: every
// destination was under sixty-four and every offset under 0x141, so a narrow field
// explained them all.
//
// This is the third time a field has solved too narrow for want of a high sample, and
// the second time in this file. The pattern is not that the lesson is hard; it is that
// adding an opcode means adding its extremes, and the extremes are the part that is
// easy to leave until the layout looks plausible.
//
// Destinations reach the top of the file, offsets reach the top of the field, and bases
// pass one hundred so a six-bit base and a scaled reading are both ruled out.
s_load_dwordx2 s[96:97], s[100:101], 0xfffc
s_load_dwordx2 s[70:71], s[86:87], 0x8000
s_load_dwordx2 s[34:35], s[100:101], 0x7ffc
s_load_dwordx2 s[100:101], s[70:71], 0x1234
s_load_dwordx4 s[96:99], s[100:101], 0xfff0
s_load_dwordx4 s[64:67], s[86:87], 0x8000
s_load_dwordx4 s[36:39], s[100:101], 0x4444
s_load_dwordx4 s[80:83], s[70:71], 0x7ff0
s_load_dwordx8 s[88:95], s[100:101], 0xffe0
s_load_dwordx8 s[64:71], s[86:87], 0x5550

// And high offsets on the single-word load too, since it shares the field.
s_load_dword s101, s[100:101], 0xfffc
s_load_dword s64, s[86:87], 0x8000

// ---- wide flat memory ------------------------------------------------------------
//
// Same fields as the single-word forms, differing only in how many consecutive registers
// they carry. Probed rather than assumed to share a layout, because "differs only in
// width" is exactly the assumption that produced four disagreeing scalar loads earlier
// in this file.
//
// Addresses reach the top of the register file, and each source gets its own high
// sample - both traps this file has already fallen into once.
global_load_dwordx2 v[0:1], v2, s[4:5]
global_load_dwordx2 v[200:201], v255, s[100:101]
global_load_dwordx2 v[100:101], v190, s[70:71]
global_load_dwordx4 v[4:7], v8, s[6:7]
global_load_dwordx4 v[248:251], v255, s[100:101]
global_load_dwordx4 v[128:131], v77, s[70:71]
global_store_dwordx2 v0, v[2:3], s[8:9]
global_store_dwordx2 v255, v[200:201], s[100:101]
global_store_dwordx2 v190, v[100:101], s[70:71]
global_store_dwordx4 v1, v[4:7], s[10:11]
global_store_dwordx4 v255, v[248:251], s[100:101]
global_store_dwordx4 v77, v[128:131], s[70:71]

// ---- local data share, with its byte offset --------------------------------------
//
// Every earlier `ds_` probe used the offset-free form, so the offset field was never in
// any sample and the solved layout has no slot for it - a translator built on that would
// silently ignore every `offset:` a compiler emitted and read the wrong word. Same shape
// as the flat accesses, where the no-base form hid the base entirely.
ds_read_b32 v1, v2 offset:16
ds_read_b32 v100, v200 offset:65535
ds_read_b32 v255, v190 offset:1
ds_read_b32 v9, v77 offset:32768
ds_write_b32 v3, v4 offset:32
ds_write_b32 v200, v255 offset:65535
ds_write_b32 v190, v12 offset:1
ds_write_b32 v44, v88 offset:32768

// Untyped buffer access (MUBUF). Its operands sit in the second word and its address is
// assembled from several of them at once - a resource constant in four scalar registers,
// an offset register, an index register, a scalar offset and a literal - so which fields
// are live depends on the `offen` and `idxen` bits rather than on the opcode alone.
//
// Probed with the address modifiers varied deliberately: `offen` alone, `idxen` alone,
// and both, because a solver shown only one combination cannot tell an operand field
// from a modifier bit that happens to be constant.
//
// Some registers are deliberately **above 127**, so the top bit of the data field is
// exercised. Without one the solver made that field seven bits wide and handed the
// eighth to the resource field beside it - the two are adjacent, and a field that is
// always zero is indistinguishable from a field that is not there. The product came out
// right for every sample given and would have been wrong for the first shader using a
// high register: the same too-narrow-field fault this project has hit four times before.
//
// The register numbers are also deliberately **uncorrelated**. A first pass numbered every
// destination one below its address register - v1/v2, v11/v12 - and the load would not
// solve at all: with `dst == addr - 1` in every sample there is nothing to tell the two
// fields apart, and the solver refused rather than picking one. The store solved only
// because its samples happened to break the pattern.
buffer_load_dword v1, v40, s[8:11], s3 offen
buffer_load_dword v37, v2, s[16:19], s13 offen
buffer_load_dword v21, v55, s[24:27], s33 offen
buffer_load_dword v60, v9, s[32:35], s7 offen
buffer_load_dword v14, v14, s[40:43], s19 offen
buffer_store_dword v4, v51, s[12:15], s6 offen
buffer_store_dword v45, v5, s[20:23], s16 offen
buffer_store_dword v24, v63, s[28:31], s26 offen
buffer_store_dword v58, v11, s[36:39], s36 offen
buffer_load_dword v41, v6, s[44:47], s43 idxen
buffer_load_dword v3, v52, s[48:51], s53 idxen
buffer_load_dword v61, v18, s[52:55], s63 idxen
buffer_store_dword v44, v13, s[56:59], s46 idxen
buffer_store_dword v7, v55, s[60:63], s56 idxen
buffer_store_dword v64, v29, s[64:67], s66 idxen
buffer_load_dword v71, v[8:9], s[68:71], s73 idxen offen
buffer_load_dword v12, v[82:83], s[72:75], s83 idxen offen
buffer_store_dword v74, v[15:16], s[76:79], s76 idxen offen
buffer_store_dword v19, v[85:86], s[80:83], s86 idxen offen
buffer_load_dword v200, v17, s[84:87], s21 offen
buffer_load_dword v131, v240, s[88:91], s31 offen
buffer_load_dword v255, v128, s[92:95], s41 idxen
buffer_store_dword v199, v23, s[96:99], s51 offen
buffer_store_dword v144, v250, s[68:71], s61 offen
buffer_store_dword v250, v130, s[72:75], s71 idxen
// A scalar offset given as an inline constant rather than a register: its operand code is
// 128 and up, which is the only way the top bit of that field is ever set. Without one it
// solved seven bits wide where the reference says eight - the same fault as the data
// field above, one field along.
buffer_load_dword v3, v44, s[8:11], 0 offen
buffer_load_dword v46, v7, s[16:19], 0 idxen
buffer_store_dword v9, v33, s[24:27], 0 offen
buffer_store_dword v52, v18, s[32:35], 0 idxen
