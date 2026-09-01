# The target generation was never checked


Every table in this thread is derived against `gfx900` - the fifth generation of the
architecture - because that is what `MCPU` is set to in the fixture and probe generators.
The encoding families, all sixty-two operand layouts, the 126-instruction differential
corpus and every mnemonic come from assembling and disassembling for that target.

Nobody checked whether it is the right one. Measured against a later generation:

| | |
|---|---|
| Probe instructions assembling on gfx900 | 322 |
| Rejected outright on gfx1030 | 69 (21%) |
| Encodings differing in `scalar.s` alone | 62 of 120 (52%) |

`v_add_f32_e64` is `0xD1010000` on one and `0xD5030000` on the other - a different family
value *and* a different opcode number. `v_cmp_lt_f32_e32 vcc, …` does not assemble at all
on the later target, because its thirty-two-lane mode makes a comparison destination
thirty-two bits wide.

**This is the deepest version of the pattern this session keeps finding.** A table that is
plausible, internally consistent, and *differentially verified* - against the wrong
reference. The differential test would pass forever, because it compares against the same
disassembler the tables were derived from. Self-consistency is not evidence, and neither
is agreement with the thing you copied.

**The cost is far below what 52% suggests, and for a reason worth naming.** The tables are
**generated, not transcribed**. `MCPU` is one constant; re-running the generators produces
new tables and the differential test re-verifies them. D097 chose to solve rather than
transcribe on the grounds that a solved table can be checked; the larger payoff turns out
to be that a solved table can be *retargeted*.

**What would not carry over.** The translator keys on `(family, opcode number)` across
about sixty match arms. Opcode numbers move between generations; mnemonics do not. Keying
on the mnemonic makes the generation a configuration choice rather than something baked
into code - principle 5 applied to the one place it was not.

**Not resolved here.** Which generation the target actually is, is a question for the side
of the project holding the binaries. It also decides the wavefront width, which is the
central parameter of the subgroup fidelity model - so that model waits rather than being
built against an assumption that has just been shown to be load-bearing and unchecked.

