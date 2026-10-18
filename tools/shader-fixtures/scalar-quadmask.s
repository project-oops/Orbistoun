// The quad mask, which no compiled fixture reaches: each bit of the result whether any lane of a
// group of four is set, as a pixel shader widens its mask to whole quads.
//
// Grand Theft Auto V's scene pixel shaders do `s_quadmask_b64 vcc, vcc` (SOP1 opcode 45 had no
// recorded name).
//
// Written by hand and assembled, as `unreached.s` is: the reference still decides the bytes and
// the names.
s_quadmask_b64 vcc, vcc
s_quadmask_b64 s[4:5], s[6:7]
s_quadmask_b64 s[100:101], exec
s_endpgm
