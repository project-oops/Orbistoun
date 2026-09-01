# D239 - Two counters, one quantity, and the number a person read was wrong by ten


**decided** · 2026-08-25 · found by an external review, reproduced here

`orbistoun-cli knows` printed **80** open questions. `orbistoun-cli questions` printed
**70**, of the same knowledge base, in the same build. `PROJECT_STATUS.md` said 76. Neither
line said which definition it meant.

The cause is two implementations of one rule:

- `FunctionKnowledge::open_questions` charged an `assumed` entry a whole-function penalty
  **plus** each of its itemised assumptions.
- `cmd_questions` listed the assumptions and added the penalty **only when nothing was
  itemised**.

There are 10 assumed functions and 70 listed assumptions, and all 10 itemise - so one said
10 + 70 and the other said 70.

**The second rule is the right one, and both comments already described it.** An entry
resting on a guess and listing nothing still counts as one, so a total cannot be shrunk by
leaving the detail out; an entry that itemises is already counted by its items. Charging
both makes a candid entry cost more than a vague one *for being candid*, which is the
opposite of what either comment claimed to want.

`open_questions_asked()` now returns the questions, `open_questions()` is its `.len()`, and
the reporting shim asks the entry instead of assembling the list itself. They cannot
disagree because there is one of them.

### The test that guarded it was a third copy

`the_open_question_count_matches_what_the_entries_carry` re-implemented the counting rule
and asserted the sum agreed. That is not a guard - it is a third copy of the definition,
and it passed for exactly as long as two of the three matched. It sums what the entries
*would print* now, and separately asserts the invariant with teeth against the real
knowledge base: an entry that itemises contributes its items and nothing more.

There turned out to be **four** copies of the rule, not two. Besides the two counters and
the guard test, `orbistoun-probe`'s conformance test asserted an entry's open questions were
`2` with the message *"the whole entry, plus one stated"* - the double count written out in
words and frozen as an expectation. It now asserts the count equals the number of
assumptions the entry lists, which is the property rather than the number.

**The class matters more than the count.** This is principle 3's addendum - a guard, a
verdict or a report is as capable of plausible output as a stub is - occurring inside the
honesty machinery itself. It is the third instance today, after two eliminations that were
diagnostics which never ran (D229, D230).

### A citation naming a path is not a citation

`sceKernelCreateSema` cited `C:	emp\obscene-orbistoun-bridge.md`. The file exists, is
owned by neither repository, and can be resolved by no reviewer and no CI job - so the one
thing `cites` exists for, letting somebody else check the claim, was defeated by the value
in the field.

Downgraded to what it actually rests on: obSCEne's `platform.h` marked `OBS_FROM_SPEC`, and
obSCEne D154. A named document in a sibling repository travels the way "ISO C 7.21.6.5"
travels; a location on one machine does not.

The provenance audit now refuses a `cites` value containing a path - a drive letter, a
leading slash, a relative prefix. **Made to fail before being trusted**: the rule was added
first and the existing entry tripped it, which is the only reason to believe it works.

