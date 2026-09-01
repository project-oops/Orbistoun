# D315 - A submission is a bundle of claims, and the receiver counts them itself


**decided** · 2026-08-27 · the missing half of D297, built once both slots existed

The loop does not need this repository. That is the property worth the most and the one
least served by the tooling: measurements land in a data directory under somebody's profile,
title results in whatever directory they ran from, and **no command said *here is what this
machine has to contribute***. For a checkout that is a convenience - the files are in the
tree and a diff shows them. For somebody running a binary it is the difference between
contributing and not.

`orbistoun-submit` holds what a submission is. Two kinds of claim and nothing else:

| carried | why |
|---|---|
| measurements | what a function must answer, and what that rests on |
| title results | how far one title got, and under which policy |

Both are derived from running a binary the submitter owns, reproducible by anyone with the
same title, and falsifiable by a command. **Traces and run reports are excluded** - they are
inputs rather than claims, they are large, and they carry far more of a title than a result
needs to. A submission should be readable by whoever receives it.

**It could not have been built before today.** A mining run is a run with a measured policy,
and `compat record` refused those outright - so a distributed contributor had to pass
`--force`, and their entry then contaminated the honest number. The two slots are what make
their result carryable at all (D312).

The crate depends on the measurement format and the title record and **nothing else**. No
loader, no emulator, no model runtime, enforced by cargo rather than by care (principle 12):
a bundle carries claims and cannot smuggle behaviour.

### The bug it shipped with, found by running it

`submit check` printed *"6 title result(s)"* from a bundle carrying seven. It read the count
off the manifest.

A manifest is a claim by whoever sent it. Quoting it back reports the **sender's arithmetic
as the receiver's measurement**, which is this log's oldest failure arriving in the newest
code - and it landed in the one command whose entire job is to not trust what it was handed.
The receiver now counts the files and says so when the manifest disagrees.

Worth stating plainly: that was caught by running the command against a hand-edited bundle,
not by writing it carefully. Every guard in this file is now one somebody has watched refuse
something.

