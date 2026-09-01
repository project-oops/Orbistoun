# The condition code, everywhere it was missing


Scalar integer arithmetic and the compact forms translate: `s_add_i32`, `s_sub_i32`,
`s_and_b32`, `s_or_b32`, `s_xor_b32`, `s_movk_i32`, `s_cmpk_eq_i32`, `s_cmpk_lg_i32`,
`s_addk_i32`, `s_mulk_i32`. The census went from 93 to 102 of 126 instructions and from 2
to 3 of 10 fixtures complete. Fifty-five execution tests on a real device.

**The unit started as breadth and turned into a correctness fix.** The 64-bit scalar
logic was translated three units ago and had been dropping its condition-code write the
whole time. `s_and_b64 exec, exec, vcc` followed by a branch on the code is how a
compiler skips a block once no lane survives, so a shader doing that branched on whatever
the previous compare had left there.

Nothing here could have caught it. The behaviour is invisible in the encoding, invisible
in the operand layout, and invisible to every test that checks destinations - which is
all of them. It is documented in the published instruction set and nowhere else this
project can reach. **A hidden side effect has to be looked up; no oracle available here
will volunteer one.** The differential test finds wrong encodings and the solver finds
wrong field widths, and neither has anything to say about what an instruction does
besides write its destination.

The regression test was checked by removing the fix and confirming it fails. That habit
started with the family-agreement check two units ago and has now caught something twice.

**Surprises.**

- **The family is not uniform about what the code means.** The logical operations set it
  from whether the result is non-zero; the arithmetic ones from signed overflow; and
  `s_mulk_i32` does not set it at all, alone among the compact forms. Guessing one rule
  and applying it everywhere would have been wrong in two directions at once, and the
  test for the arithmetic case explicitly says why the non-zero rule would agree
  whenever the sum happened to be zero.

- **`s_addk_i32` and `s_mulk_i32` read their destination.** They accumulate rather than
  assign, so translating either as a move leaves a shader computing from whatever was in
  the register - and the answer looks like a number either way.

**Blocked on the other thread, not on me.** The workspace gate is currently red on
`crates/orbistoun-elf/examples/dyntags.rs`, which is untracked and belongs to the loader
work. Every crate this side owns lints clean and passes, with no device skips. Not
touched, deliberately.

**Not done.** The division helpers are the last VOP3 blockers and are still deliberately
left - the Newton-Raphson sequence has special-case behaviour around denormals and
overflow that deserves care rather than a first guess. `Fidelity::Subgroup` is still a
stub. Everything else outstanding needs the resource model, and that needs a submission
from a title.

