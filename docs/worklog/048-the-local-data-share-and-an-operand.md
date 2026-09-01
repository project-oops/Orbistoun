# The local data share, and an operand that was never in any sample


`ds_read_b32` and `ds_write_b32` translate. The census went from 104 to 105 of 126 and
from 4 to 5 of 10 fixtures complete - half the corpus. Sixty-nine execution tests.

**The instruction was the easy part; its offset was not.** The byte offset lives in the
first word, and *every earlier probe used the offset-free form* - so the field appeared in
no sample, and the solved layout had no slot for it. A translator built on that ignores
every offset a compiler emits and reads the wrong word, with a perfectly valid address
register and nothing to complain about.

Probing with offsets then made the opcode unsolvable, which was the good failure: the
reference appends `offset:16` to the last operand with no comma, so splitting on commas
left `v2 offset:16` as one piece that matches no register pattern. The solver now treats a
named immediate as the operand it is.

This is the flat accesses' no-base trap again, in a new costume: **a probe set that omits
a case teaches the solver the case does not exist.** Third occurrence. The generalisation
already written down after the second one was correct and did not prevent the third,
because the shape it takes changes each time - a hidden base, then a hidden width, now a
hidden field appended without punctuation.

**Surprises.**

- **The DS opcode sits at bit 17**, where the flat families use 18. A test helper with the
  wrong shift produced a word that decoded as a *different* local-share instruction, which
  reported as a missing operand layout rather than as a wrong opcode - so the error named
  the table rather than the caller.

- **Workgroup storage takes one index where a storage buffer takes two.** The buffers next
  door are a struct containing an array and need the member first; this is a bare array
  and does not. Confusing the two faulted a driver earlier in this crate's life, and
  writing it correctly here was a matter of remembering that rather than of anything the
  compiler could check.

**Instruction breadth is now exhausted.** Every remaining blocker needs either the
published instruction set (the three division helpers) or the resource model (`exp`,
MTBUF, MUBUF, MIMG, VINTRP - sixteen of the twenty-one). There is no further unblocked
translation work in this thread until a capture arrives.

