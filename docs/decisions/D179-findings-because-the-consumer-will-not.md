# D179 - Findings, because the consumer will not be a person


**decided** · 2026-08-21

Every diagnostic this project has was built for a specific bug and rendered as prose for
someone reading a terminal: the ranked import list, the fault, the call tail, the stack
conformance line, the read counts. Reading them takes a person who knows what each shape
means, and **that person is the bottleneck**.

The stated end goal is an emulator that is loud and clever about what it dumps - one that
debugs as it goes, and eventually proposes its own fixes. That consumer is not a person. It
needs the same four things every time, as data: **what is wrong, where, what evidence says
so, and what would address it.**

So a run now produces *findings* rather than output. Each carries a machine-routable kind,
a subject, evidence taken from the trace, a suggested action, and a weight - ranked so that
taking the top item is taking the one least likely to waste the reader's time.

### Confidence is the load-bearing field

**A confidently wrong suggestion is worse than no suggestion, because it gets acted on.**

That is not a general worry, it is this project's own history. An entry convention that
looked right for months. A stub policy that looked wired to something. A name sweep whose
vocabulary could not contain the answer it was looking for. Each would have produced a
confident, wrong finding, and an automated consumer would have acted on all three.

So nothing reports `Certain` unless the trace *shows* it rather than suggests it, and the
rule is the one obSCEne already uses: a certain finding is a defect, a possible one is a
conversation. Ranking is by confidence **before** weight - a heavy guess must never outrank
a light certainty.

### What it detects, and why these

| gap | why it is worth a category of its own |
|---|---|
| `Unimplemented` | The clearest instruction there is: names a function, says how much the guest leaned on it, and the work is unambiguous |
| `ErrorUsedAsPointer` | The most productive signal this project has - names the function that answered wrongly *and* proves the guest believed it |
| `GuestGaveUp` | The guest reporting its own reason, which no other signal offers |
| `Spinning` | Distinguishes "not progressing" from "failed", which look identical in a call count |
| `Unnamed` | A naming job, not an implementation one - "implement `libkernel::0xcedb…`" is not an instruction anyone can follow |
| `AbiViolation`, `ShortRead` | Contract failures that fault far from their cause |

The placeholder match deliberately allows a small offset either side. A guest that treats an
error code as a struct pointer reads a *field* through it, so the address that faults is the
code plus or minus a little - matching the bare value would miss every case where the guest
did anything with it (D125).

`Unimplemented` needed one new fact: whether a called import has a handler behind it.
Without it a trace cannot tell "the guest used this and it worked" from "the guest used this
and got a placeholder" - opposite conclusions drawn from the same line.

### It reproduced a day's work in one pass

Pointed at the corpus, it independently classified all three failure shapes this session
found by hand: the spin on `sceKernelDirectMemoryQuery`, the placeholder dereferenced by
PPSA21564, and the deliberate abort in two others - with the error-reporting call that
precedes the abort surfaced as its evidence.

Which is the point. Those took a day of manual reading; the run now says them itself.

