// Typed buffer access, for solving MTBUF's operand layout.
//
// # What makes these different from the untyped probes
//
// A typed access carries a *format* saying how to convert what it fetched - the untyped
// ones move raw dwords. The format is an encoding field rather than an operand, so the
// reference prints it as `format:[BUF_FMT_...]` after the last operand and the solver
// skips it, the same way it skips `offen` and `idxen`. It is varied here anyway, so the
// samples look like real ones and a solver that mistook the format bits for part of an
// operand field would have somewhere to go wrong.
//
// # Why the registers look arbitrary
//
// They are chosen to be *uncorrelated*. An earlier probe set gave every load a
// destination one below its address register, which makes the two fields indistinguishable
// - any candidate explaining one explains the other with an offset - and the opcode
// reported as unsolvable with nothing to say why. So the data register, the address
// register, the resource base and the scalar offset vary independently here.
//
// The resource base steps by four because a buffer resource constant occupies four
// consecutive scalar registers and its field holds the group index rather than the
// register number.
//
// That last point is why every opcode below needs at least one data register above 128,
// and it is not obvious. The resource field is five bits at 16 holding the group index,
// so the register it names is `(word >> 16 & 0x1F) * 4` - which is arithmetically
// identical to `(word >> 14) & 0x7C` whenever bits 15:14 are zero. Those bits are the top
// of the *data* register field. Keep every data register below 64 and the false reading
// fits every sample forever, the true one fits too, and the opcode reports as unsolvable
// with nothing pointing at the cause.
//
// Four of these five opcodes failed exactly that way on the first run. The one that
// solved was the one that happened to have a `v200` in it.
//
// The resource bases also vary in whether the *group index* is odd or even, for the same
// class of reason one bit along. The index is `base / 4` and it sits at 20:16, so its low
// bit is bit 16 - which is also the top of a nine-bit window over the data register at 8.
// Make every group index odd and that bit is always 1, so the window reads `v4` as 260,
// which is exactly `v4`'s code in the shared source numbering: an eight-bit vector
// register and a nine-bit source explain every sample and nothing can separate them.
//
// Two more opcodes failed that way on the second run, after the first cause was fixed.
// The bases here are deliberately a mix of multiples of eight and of four alone.
//
// Finally, the address registers and scalar offsets reach past 128 and 64 respectively.
// Without that, both fields solve narrow and get widened from the rest of the family - a
// value inherited rather than measured. The generator says so when it happens, and this
// is what taking its advice looks like.
//
// One sample per opcode uses a literal `0` for the scalar offset, which is not a register
// at all - it is inline constant 128 in the shared source numbering. Without it the field
// solves seven bits wide, because no scalar register this file names reaches 128. Nothing
// inside the family disagrees, so nothing flags it: every typed-buffer opcode solves the
// same narrow width and the reconciliation has nothing to compare. The disagreement is
// with the *untyped* family, which solves the same field at eight, and no check looks
// across families. Real shaders use `0` here constantly.
//
// Multi-channel forms take a register *range* for their data, and the range's base is
// what the field encodes.

// One channel, loading.
tbuffer_load_format_x v1, v40, s[8:11], s3 format:[BUF_FMT_32_UINT] offen
tbuffer_load_format_x v37, v202, s[16:19], s93 format:[BUF_FMT_32_FLOAT] offen
tbuffer_load_format_x v21, v155, s[24:27], s73 format:[BUF_FMT_8_UINT] offen
tbuffer_load_format_x v60, v9, s[32:35], s7 format:[BUF_FMT_32_UINT] idxen
tbuffer_load_format_x v14, v29, s[40:43], 0 format:[BUF_FMT_32_FLOAT] idxen
tbuffer_load_format_x v72, v6, s[44:47], s43 format:[BUF_FMT_8_UINT] idxen
tbuffer_load_format_x v200, v17, s[84:87], s21 format:[BUF_FMT_32_FLOAT] offen

// Two channels. The data operand is a pair, and its base is what the field holds.
tbuffer_load_format_xy v[2:3], v244, s[12:15], s96 format:[BUF_FMT_32_32_FLOAT] offen
tbuffer_load_format_xy v[46:47], v5, s[16:19], s16 format:[BUF_FMT_32_32_FLOAT] idxen
tbuffer_load_format_xy v[130:131], v163, s[28:31], s76 format:[BUF_FMT_32_32_FLOAT] offen
tbuffer_load_format_xy v[58:59], v11, s[32:35], s36 format:[BUF_FMT_32_32_FLOAT] idxen
tbuffer_load_format_xy v[188:189], v70, s[52:55], 0 format:[BUF_FMT_32_32_FLOAT] offen

// Four channels.
tbuffer_load_format_xyzw v[8:11], v212, s[20:23], s95 format:[BUF_FMT_32_32_32_32_UINT] idxen
tbuffer_load_format_xyzw v[52:55], v18, s[48:51], s53 format:[BUF_FMT_32_32_32_32_FLOAT] offen
tbuffer_load_format_xyzw v[128:131], v193, s[56:59], s84 format:[BUF_FMT_32_32_32_32_UINT] offen
tbuffer_load_format_xyzw v[192:195], v25, s[60:63], s56 format:[BUF_FMT_32_32_32_32_FLOAT] idxen
tbuffer_load_format_xyzw v[100:103], v7, s[64:67], 0 format:[BUF_FMT_32_32_32_32_UINT] offen

// Storing, one channel.
tbuffer_store_format_x v4, v251, s[12:15], s86 format:[BUF_FMT_32_FLOAT] offen
tbuffer_store_format_x v45, v5, s[24:27], s16 format:[BUF_FMT_32_UINT] idxen
tbuffer_store_format_x v132, v233, s[28:31], s74 format:[BUF_FMT_8_UINT] offen
tbuffer_store_format_x v58, v11, s[40:43], s36 format:[BUF_FMT_32_FLOAT] idxen
tbuffer_store_format_x v201, v55, s[60:63], 0 format:[BUF_FMT_32_UINT] offen

// Storing, four channels.
tbuffer_store_format_xyzw v[24:27], v228, s[28:31], s87 format:[BUF_FMT_32_32_32_32_FLOAT] idxen
tbuffer_store_format_xyzw v[44:47], v213, s[56:59], s94 format:[BUF_FMT_32_32_32_32_UINT] offen
tbuffer_store_format_xyzw v[168:171], v9, s[68:71], s73 format:[BUF_FMT_32_32_32_32_FLOAT] offen
tbuffer_store_format_xyzw v[212:215], v82, s[72:75], s83 format:[BUF_FMT_32_32_32_32_UINT] idxen
tbuffer_store_format_xyzw v[80:83], v19, s[76:79], 0 format:[BUF_FMT_32_32_32_32_FLOAT] offen
