# D111 - Function bodies are exempt from the define-before-use check


**Status:** decided (2026-08-21) - both halves now have tests where the narrowing happened

`Builder::check` verifies define-before-use in the declarations section and only
"defined somewhere" in the function section.

Forced by control flow: a branch names a label appearing later, and a loop header names
its own merge block before either block exists. Both are legal and necessary, so the
ordering half of the check cannot apply there.

Recorded rather than quietly relaxed because it narrows a check written two units ago in
response to a driver fault. The half that caught that fault - an identifier reserved and
never given a meaning at all - still applies everywhere, and the declarations section,
which is where that bug lived, keeps both halves.

### The narrowing was untested where it was made

The declarations section had tests for both halves. The function section, which is the
part this decision *changes*, had none - so the exemption was asserted rather than
demonstrated, and the half deliberately kept was not being exercised anywhere it mattered.

Two tests now: a forward branch to a block written afterwards is accepted, and a branch to
a block written nowhere at all is refused by name. The second is the one that matters -
relaxing the ordering rule inside a function is forced by control flow, but relaxing it
into "anything goes" would give back exactly the fault the check was written for.

Worth noting what the retained half has since caught elsewhere: a malformed `OpTypeVector`
whose shape entry listed its own result as one of its uses, found in the declarations
section before a driver saw it.

