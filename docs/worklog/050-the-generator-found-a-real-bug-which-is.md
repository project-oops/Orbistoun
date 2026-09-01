# The generator found a real bug, which is what it was for


Both of the remaining unblocked items are done. 209 tests across five crates.

**The generator was widened** from twelve instruction forms to twenty-six, covering the
long-form arithmetic, the compact scalar forms, the scalar compares, wide flat memory and
the scalar loads. All forty-eight generated programs now translate and compare.

**It found a wrong opcode number on the first widened run.** `v_sub_f32_e64` is 258 and
`v_subrev_f32_e64` is 259; the supported list said 259 and 260. So every long-form
reverse-subtract computed `a - b` where the guest means `b - a`, and the instruction one
further along was translated as something it is not.

Nothing had caught it. The short-form subrev has a test and passes, because the short form
is a different opcode and correct. The long form had no test of its own - and the numbers
were assumed from the short form's ordering rather than read off the solved operand table
sitting in the same repository. The generator surfaced it indirectly, by emitting an
opcode with no operand layout, which is a cheaper signal than a wrong pixel a year from
now. There is now a test pinning both directions.

**Guest memory needed a bound first.** Widening the generator produced addresses the
hand-written tests never did, and an index past the end of the array is undefined
behaviour in the emitted module - so the two models were not *required* to agree there and
any comparison was comparing a coincidence. Masking makes it defined and identical (D137).
The semantics are still wrong: an address outside its mapping should be refused, which
needs a real mapping to refuse against.

**Decoder robustness** is nine tests over random bytes, all zeros, all ones, truncated
instructions, ragged lengths, an empty buffer, and every value of the high byte that
selects an encoding family. The properties are that it terminates, does not panic, offsets
strictly increase and stay inside the buffer, and garbage is reported as untrustworthy
rather than presented as a program.

Termination turned out to be guaranteed by construction rather than by these tests: the
table loader already refuses a zero or non-multiple-of-four width, and the unrecognised
path advances by four. That refusal is now pinned by a test of its own, so the guarantee
stays a guarantee rather than an accident of what the table happens to contain.

The pipeline got the same treatment - arbitrary command streams, truncated ones, and a
registration pointing at bytes that are not a shader. A crash there is the emulator dying
on a frame it could have skipped.

**Surprises.**

- **The most useful failure was the least dramatic.** "No operand layout" for VOP3:260 is
  a dull message, and it was the thread leading to a silently wrong subtraction. The
  generator did not find the bug by executing it; it found it by producing an instruction
  the tables disagreed about.

- **A test was written and deliberately not written.** The generator emits no branches: a
  generated backward branch is a hung GPU rather than a red test, and that is the one
  place in this session where the careful version was not worth building (D138).

