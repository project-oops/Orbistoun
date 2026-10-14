# D850 - A lane run's addresses are found natively, checked against the model

**Status:** assumed
**Date:** 2026-10-14

A primitive shader's global-load addresses (D758) are found by a native run where the program
allows it: the program, through its last load, is compiled once into steps over plain words for
the instructions such programs hold - moves, compares, scalar loads, branches the run can decide,
the lane count, integer adds and multiplies and the 64-bit add with carry - and run per draw. A
program holding any instruction outside that set, a long form carrying a modifier, or a kept
instruction that reads across lanes is not compiled; a run needing a value it does not know, or
taking a branch past the program's words, declines. Either way the model's run (D758) answers, as
it did before.

Each program's first 64 native answers are checked against the model's. One that differs stops the
native run for that program, with both answers in the log; after 64 that agree, the native answer
is taken alone, and a draw it answers skips the kept-answer table, whose lookup costs more than the
run.

**Why:** the model emits every instruction as SPIR-V and evaluates the emission, about 5 us a run.
A title drawing a triangle at a time with its first vertex in user data (Ship of Harkinian, and the
other Fast3D ports) runs it every draw, since no kept answer ever matches: a quarter of Ship of
Harkinian's run was resolving draw buffers. The native run takes 0.26 us; the bind went from 6.5 to
0.9 us a draw, 24% of the run to 3-4%.

**Rejected:**
- Keying the kept answers on less of the user data: which words a run depends on is what running
  it finds out.
- An affine model of each load's address solved once per program: exact only where every step is
  affine, which the program's branches on descriptor words are not, and a second semantics larger
  than this one.
- Trusting the native run without the check: its semantics are a second copy of a handful of the
  model's, and a disagreement would bind a wrong range silently.
