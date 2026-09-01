# D299 - Finding which answer the guest dereferenced, rather than reasoning about it


**decided** · 2026-08-26 · `PPSA21564` dies writing to `0x7fff0001`

Turning the loop on all five commercial titles produced one clean result and four refusals, and
the refusals are more useful than the success. `PPSA21564` faults at **`write to 0x7fff0001`** -
which is `GuestError::Unimplemented`, one of our own placeholder codes, being used as an
address. The guest asked something, got "not handled", and dereferenced it.

D125 already states what the fix is: *"for anything the caller dereferences, an error code is a
wild pointer - so those answer zero, which is what a caller already tests for."* And the
inference is **measured rather than assumed**: a guest treating an answer as an address is the
evidence that the function returns something dereferenceable. Nothing has to be recalled about
what the function is for.

**What is missing is which function.** The report produces two `ErrorUsedAsPointer` findings and
neither names the producer: one names the call that *received* the placeholder, the other names
the import the fault happened inside. The action says so out loud - *"find what answered with
that code just before"* - which is an instruction to a person to go looking.

Looking is a sweep. Every import the run called that nothing implements is a candidate; force
each to answer zero in turn and see which one stops the fault being a placeholder dereference.
The oracle is crisp and needs no judgement: **the faulting address stops being one of our own
codes**, or it does not.

Two things make this cheaper than it sounds. The candidates are already in the trace, so the
list costs nothing to build. And a boot is about a tenth of a second, so a title with a dozen
unimplemented imports is under two seconds - the same measurement that made every other sweep
here exhaustive rather than ranked (D231).

**And its patch is auto-keepable, which none of the others are.** It changes what a function
*answers* and writes no memory, so `Evidence::Further` is sufficient by the rule set in D296 -
a wrong answer that buys progress shows up as a wall that moved, where a wrong write does not
show up until something unrelated breaks.

