# 2026-09-01 (/loop) - GNM dispatch header now byte-matches hardware (D439 cont.)


obSCEne's 165-gnm/dispatch-direct captured the exact PM4 bytes: hardware's DISPATCH_DIRECT header is
0xc0031502, orbistoun wrote 0xc0031500 - missing bit 1, the shader-type bit that routes the packet to
the compute pipe (a dispatch is always compute). Set it; orbistoun's dispatch packet now byte-matches
hardware's first 5 dwords. The 6th dword the check counts (res 6 vs 5) is the surrounding hardware state
orbistoun deliberately does not model, not part of this packet (documented). gpu tests green (26+17+4).

Turn total: D438 test fix + audio close + 5 file error codes + GNM dispatch header, all verified byte/
value-exact against the hardware report. Error-code sweep now covers 000/015/020/040(x5)/090/100 + the
gnm header. Remaining diff items (D439): neo-mode (machine-model: base vs faster-revision, a judgement),
memory-map region count (the deeper map-shape question, D083), mutexattr round-trip 4vs5, sysmodule-query
0x805a1000, dlsym handle validation, and the 0x2_0000_0000 map base (Windows test) - each needs its own
investigation, not a value swap.

