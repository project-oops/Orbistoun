# 876. A peek can start from a fault register

**2026-09-26**. `ORBISTOUN_PEEK`'s indirect form `[<slot>]+len` could name only a fixed address.
PPSA02664's binding object is on the heap and moves between runs (`0x740001ec78a0`,
`0x740001cc78a0`, `0x7400020478a0`), but it is always in `r15` at the fault. So neither the
object nor a fixed slot reached it.

A slot can now be a register at the fault plus a hex offset: `[r15+0x8]+0x100` dumps what the
object's `+0x08` field points at, whatever the run. An unknown register name is refused rather
than read as zero. Tests: `an_indirect_peek_takes_a_register_base`, watched failing with unknown
names resolving to zero, and the existing indirect-peek test, updated for the new signature.

First use: PPSA02664's sub-table at the fault holds one entry each in tables 0, 1 and 3 (registers
0, 4 and 8), all below the extended threshold. The next step is to see how the upload routine
reaches an extended index from that table.
