# orbistoun-thunk

Per-import thunks: the machine code a guest lands on when it calls out, and the dispatch
behind it.

**Models:** one thirty-two byte stub per import, the shared trampoline they jump to, the
handler table, argument recording, stack-alignment checking, and the fixed-size ring the
run report is built from.

**Deliberately fakes:** nothing. A call either reaches a real implementation or is
answered by the stub policy, and the report says which.

**Design note.** This is the other half of "interception is linking". Relocation writes an
address into a procedure-linkage slot; this is what lives at that address. There is no
hooking pass - the guest calls what the linker put there, and the linker is us.

**One stub per import, not one shared stub.** A shared target answers "did the guest call
something we have not written?" and nothing else. One per import answers **which**, in
order, with counts - which is the entire input to the loop this project is built around.
The cost is 32 bytes per import: 45 KiB for an executable importing 1,410 functions, once.

`r10` and `r11` carry the index and the trampoline address, because they are the two
registers System V lets a function destroy that are **not** argument registers. Anything
else corrupts an argument before the trampoline can save it, invisibly.

**The hot path is genuinely hot.** One title makes ninety-nine million calls through here
in twenty seconds. The handler is looked up once per call and every decision made from
that result; an extra predicate costs nothing on the six calls being investigated and a
great deal on the rest (D198).

**Status:** done, and exercised harder than anything else in the workspace.
