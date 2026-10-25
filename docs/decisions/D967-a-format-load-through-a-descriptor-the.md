# D967 - A format load through a descriptor the program computes decodes it in place

**Status:** decided
**Date:** 2026-10-25
**known_by:** published

A buffer access whose descriptor the program computes, in a device-addressed dispatch (D964),
reads that descriptor as the module runs and carries it out whole: format loads included. Nothing
of the descriptor is resolved on the host, so no table it was selected from is bound and no record
count bounds it.

**Placing.** The access is placed and bounded as the instruction set's buffer address equation
says, from the descriptor's own fields in the registers: its base, stride and record count, the
out-of-bounds mode (`OOB_SELECT`, word 3 bits 29:28), swizzling (word 1 bit 31) with the index
stride (word 3 bits 22:21) and the four-byte element GFX9 on fix, and adding the lane's id to the
index (word 3 bit 23). A scratch access is placed by the same equation (D954), and the two share
it. Swizzled and lane-indexed addressing were refused before as unmodelled (D147); here they are
carried out. Out-of-bounds mode 3 checks a swizzled access's byte rather than its offset (Mesa
`src/amd/common/ac_descriptors.c:824-833`). An untyped access places each word on its own, since a
swizzled buffer's words lie apart.

**Converting.** A format load reads the descriptor's fourth word: the format code (bits 18:12) and
the channel selects (bits 11:0). The module chooses, by the code, a layout among the formats a
format load the module is translated for converts (D738): each component's width, `x` first, and
how its bits become a number. Each component is extracted from the element at its bit and
converted by the same arithmetic a translated format load uses, the field's width read rather
than compiled in: unsigned and signed normalised, scaled, integers, IEEE halves, the packed 11- and
10-bit floats, and whole words. A normalised quotient over a maximum read at run time is refined
once by its fused remainder, since the driver divides by a run-time value through a reciprocal a
unit in the last place off (255 over 255 came back just under 1.0). A constant maximum it divides
by exactly. Each channel is then what its select names: zero, one (`1` for an
integer format, `1.0` otherwise) or a component, zero outside the buffer as a translated load
answers it. The element is read from the guest's bytes by device address, at any byte, its words
looked up in at most two chunks.

**What cannot be answered.** A format code no translated load converts (including the invalid
code), a reserved select, a select of a component the format lacks, and an element wider than four
bytes in a swizzled buffer are escapes: the lane reads zero and the dispatch's result is refused
(D950), as a refusal would have refused it before it ran. A null descriptor, all of whose selects
are `SQ_SEL_0`, reads zeros and is none.

**D966 afterwards.** A table bound whole stays where it binds: its records are read once on the
host, its accesses read bound pages rather than looking each address up, and a format load
converts by the one format the module was translated for. Where it cannot - a table of more than
256 records, or one whose records disagree in format while the program converts by format - the
dispatch is refused with the reason, and the command processor prepares it device-addressed
(D964), where this decision carries the access out. No access is refused for a table's size.

**Why:** Grand Theft Auto V's ray-traversal programs at `0x74005b857200` and `0x74005b858800`
select a buffer descriptor per lane from a table of 131072 records through waterfalls, and fetch
through it with `buffer_load_format_x`: the first at 0xd7c, 0xe0c and 0xe8c through `s[12:15]`,
the second at 33 places. The table is too large to bind whole (D966), and device addressing
carried no format, so the accesses were refused (D753, D964).

**Rejected:**
- Raising D966's bound: every record is read on the host per dispatch and its pages bound, which
  for 131072 records is the cost the bound exists to prevent, and the format must still agree.
- Translating one module per format the table's records name, chosen per lane: a lane selects its
  record as it runs, so the choice is the decode this decision makes, with a module per
  combination besides.
- A run-time table of formats in the module: the shader model has no constant arrays here, and the
  code is compared against each format's layout once per access, not per lane.
- Refusing the formats as D738 does for an unresolved descriptor: the descriptor is resolved, as
  the module runs.
