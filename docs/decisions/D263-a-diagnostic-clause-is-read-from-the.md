# D263 - A diagnostic clause is read from the right, so a target may be qualified


**decided** · 2026-08-25 · a latent bug, reported twice before it was fixed

`ORBISTOUN_WRITE` is `<import>:<slot>:<value>` and `ORBISTOUN_RETURN` is
`<import>:<value>`, both split left to right. So a label like `libkernel::sceFoo` produced
five fields where three were expected, the clause was discarded, and the run planted
nothing. Silently: a run that plants nothing looks exactly like a run that changed nothing,
and only `Finding::NeverPlanted` - which exists for this - kept twenty-three imports from
being written up as clean negatives.

The trailing fields are fixed-arity, so `rsplitn` takes them and leaves everything before as
the target. Two libraries exporting the same symbol name are now distinguishable, and
`Target::matches` already accepted a qualified label - only the parser refused it.

**Reading from the right makes the target greedy, and an existing test caught that.**
`f:0x1:0x2` parsed happily into a target of `f:0x1`, which is not a name anything exports;
a test written long before this change asserted that clause must be refused, and failed.
`is_label` restores the guard: a target is a bare symbol, a bare hash, or `library::symbol`,
so it may hold double colons and must hold no single one.

Worth noting as a small vindication of the convention that every parser gets a rejection
test. The capability and the guard are the same three-line change, and only the guard had a
test already.

