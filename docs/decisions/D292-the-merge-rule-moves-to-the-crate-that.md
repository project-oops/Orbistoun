# D292 - The merge rule moves to the crate that owns the format


**decided** · 2026-08-26 · `cmd_learn` was the only thing that knew how to record a finding

Recording a finding is a merge with rules: a field is replaced only when given, so noting one
edge case does not erase a purpose established three sessions ago; lists append without
duplicating; and an entry claiming behaviour without saying how it is known is **refused**
rather than defaulted, because every available default is a lie (D180).

All of that lived in `orbistoun-cli::cmd_learn`. Principle 13 is explicit that it should not:
*"the crates are the emulator… if a shim starts holding logic, the other two are already
drifting."* And it was about to matter rather than merely being untidy - the loop now earns
entries by measurement (D291), and a second caller would have meant a second copy of the merge
rule, including the refusal that is the whole point of it.

So `Record` and `KnowledgeFile::merge` go to `orbistoun-hle::knowledge`, which owns
`FunctionKnowledge` and `KnowledgeFile` already. The shim keeps what a shim should: where the
file is, reading it, writing it, and what to print.

**And it removes a duplicate a day old.** `turn::promote` had grown its own `Learned` struct
and a one-variant `Oracle` enum mirroring the real vocabulary - written that way to avoid
depending on `orbistoun-hle`, which was the wrong trade. A near-copy of the provenance
vocabulary is the last thing this project should carry two of, since its entire job is to make
a claim about where a fact came from checkable. `promote` returns a `Record` now.

`merge` returns the provenance faults rather than a `Result`, because they are a list of
things to say to a person and not an error to propagate - the caller is the thing that knows
whether it is a command refusing input or a loop declining to record.

