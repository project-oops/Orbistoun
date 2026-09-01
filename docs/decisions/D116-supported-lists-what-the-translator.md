# D116 - `SUPPORTED` lists what the translator handles, not what one function dispatches


**Status:** assumed

The branch opcodes are in `model::SUPPORTED` even though `model::instruction` never sees
them - the block splitter consumes them, because a branch decides where the program
counter goes and translating it in both places would be two contradictory answers.

They were left out at first, on the reasoning that the list should mirror the dispatch
match. That made the worklist rank them as blockers of a shader that already translated,
and it kept `control` - the only fixture with compiler-emitted control flow - reading as
incomplete after it had become complete. The list answers "does the translator handle
this instruction", and the answer was yes.

**The agreement test did not catch it**, which is the more useful half of this entry. It
exists precisely to stop `SUPPORTED` and the translator drifting, and it compares them
over a hand-written list of encodings that did not include any branch. A test that
enumerates its own inputs can only find drift in the cases somebody thought to enumerate.
The branches are in that list now; the shape of the hole is worth remembering.

