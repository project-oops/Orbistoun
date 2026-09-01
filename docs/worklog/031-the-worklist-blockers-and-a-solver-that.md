# The worklist blockers, and a solver that was quietly wrong about three of them


`s_load_dwordx2`, `s_load_dwordx4`, `s_load_dwordx8` and `s_mov_b64` now translate and
run. Twenty-four execution tests, all against a real device. `exp` is refused with a
reason rather than implemented - it needs a render target and there is no concept of one
(D104).

The wide loads were meant to be the easy part, and they were not.

**Their solved operand layouts were wrong, and looked fine.** `s_load_dwordx2` had solved
to a six-bit destination and an eight-bit offset; `s_load_dwordx4` to six and nine -
against `s_load_dword`'s seven and sixteen, from the same encoding with the same fields.
Three samples each, and in every one the destination was under sixty-four and the offset
under 0x141, so the narrow field explained them all.

Nothing flagged it. The generated file said `samples = 3` and gave a layout, and a
layout is a layout. It was only visible by putting the four side by side and noticing
that instructions sharing an encoding disagreed about where their fields were. After
adding high destinations, high bases and offsets at the top of the range, all four solve
identically - which is what a shared encoding should look like, and what the narrow solve
was hiding.

This is the fourth field to solve too narrow for want of a high sample, and the second in
that same file. The lesson is not hard to state; the failure is that **adding an opcode
means adding its extremes, and the extremes are the part that is easy to leave until the
layout looks plausible.**

**`s_mov_b64` was unsolvable for two reasons, and the solver could name neither.** It
reported `unsolved: SOP1:0x1` and stopped. Instrumenting it showed one operand with zero
candidates - and zero candidates means no field anywhere explains every sample, which is
the one outcome the solver cannot distinguish between "the probes are wrong" and "the
field is not there".

- `exec` was not in the operand table. A sixty-four-bit operand names its pair by the low
  half and the disassembler drops the suffix, so `exec` and `exec_lo` are the same code
  at different widths. Added as a solver-side alias rather than a second entry in the
  decoder's table, because the width is a property of the opcode and not of the code.

- The negative inline constants were missing entirely. `-1` is code 193, and the solver
  only knew the positive ones at 128 upward. One sample with no reading fails the whole
  opcode - and `s_mov_b64 s[n:n+1], -1` is the ordinary way to set a mask to all ones,
  so the sample could not simply be dropped.

The same gap existed one layer up: `read_source` refused negative inline constants
outright. A register holds thirty-two bits, so the conversion is through `i32` for two's
complement - -1 is 0xFFFF_FFFF, which is what the guest reads back.

**Surprises.**

- **`s_mov_b64` is not two `s_mov_b32`s.** A constant source is *extended*, not repeated:
  `s_mov_b64 s[0:1], -1` fills both halves, and `s_mov_b64 s[0:1], 1` sets the low half
  to one and the high half to zero. Copying the low word into both is correct for -1 and
  wrong for everything else - so a test written with -1 alone passes a wrong translation.
  Both constants are in the test for that reason.

- **A wide load can run off the end of the register file from a legal-looking encoding.**
  Registers stop at 101 and `s_load_dwordx8` takes eight, so a destination of s100 would
  write four registers and then four *specials* - and the shared operand numbering runs
  straight on, so nothing downstream would see anything wrong. Refused rather than
  truncated: a load quietly filling half its destinations is a shader computing the wrong
  thing while appearing to work.

- **The solver's silence was the expensive part.** Every one of these was minutes to fix
  and much longer to find, because "unsolved" carries no information about which operand
  or why. That is the same shape as the driver faults in the previous unit, and worth
  treating the same way when the solver is next touched.

**Not done.** `Fidelity::Subgroup` is still a stub, the Lane model is still unmasked, and
control flow with the execution mask - the largest unbuilt part of D098 - is still ahead.
SOPK, MTBUF and VINTRP remain transcribed-only.

