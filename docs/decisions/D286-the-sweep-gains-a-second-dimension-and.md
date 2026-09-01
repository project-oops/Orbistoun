# D286 - The sweep gains a second dimension, and it is a condition rather than a sentinel


**decided** · 2026-08-26 · D283 measured what D284 then had to do by hand

`experiment.rs` was written to sweep planted arguments and explicitly *not* stub returns,
and said why: *"A stub policy can change what a function **answers**. Nothing could change
what a function **does** - and both current walls turned out to be a side effect nobody
performed."*

That was half right, and the half it got right is what hid the other half. The side effect
was indeed what mattered at `image+0xafc959` - but the **return gated whether the guest ever
read it**. Answer an error and the guest takes the failure path and never looks at the
out-parameter; plant nothing and it reads a zero. Each half alone is a clean negative, and
two clean negatives read exactly like proof of absence (D283).

So `sweep` crosses the two. Three things about the shape are worth stating, because the
obvious version of each is wrong.

**The return is a condition, not a sentinel.** `RETURN_SENTINELS` exists for asking *"did the
guest compute an address from what this answered?"* - a different question, and the
differencing applies to it. Here the question is *"does answering success let the guest reach
the code that reads the out-parameter?"*, and for that only one value is interesting: the one
that means success. Sweeping return sentinels alongside argument sentinels would be a
product of two differencing questions, which is not what the wall asked.

**Unforced comes first.** The map is keyed `(slot, answer)` and `None` sorts before
`Some`, so a slot that resolves without touching the return is found and reported without
one. A finding that needs two interventions is strictly weaker than one that needs a single
intervention, and the ordering makes that automatic rather than a rule somebody applies.

**The finding carries the answer, because otherwise it is not reproducible.** "Slot 0 is an
out-parameter at offset `0xfffe0`" is false on its own - it is only true when the call also
returns zero. `Finding::OutParameter` gains the condition it was found under, so a person
reading the report can re-run it.

Cost: twelve boots become twenty-four, about 1.6 seconds to 3.2. The framing that a prior is
needed to reduce query count remains wrong, and is now wrong in two dimensions.

