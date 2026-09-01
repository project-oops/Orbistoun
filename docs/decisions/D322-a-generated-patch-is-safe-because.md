# D322 - A generated patch is safe because promotion is the verification step


**decided** · 2026-08-27 · a position corrected under a good argument

`THE_LOOP.md` says a tool *"that produces plausible implementations with no verification step
makes the codebase worse rather than better"*, and that sentence was being read here as *do
not generate implementations*. It does not say that. **The operative clause is the middle
one**, and a proposal that a person has to read, gate and merge has a verification step - a
stronger one than most code in this tree got.

So a bundle carries proposals. A patch arrives **inert**: a file, applied by nothing, which
becomes a change only when somebody promotes it. That is the same ladder `known_by` already
describes for every other fact here, and the same one D297 gives a submitted measurement.

**The real constraint was never verification. It is provenance.** Principle 1 calls a model
in the loop a third route to the convergence problem - *"this is what the function does"* can
be recalled and dressed as reasoning - and generating an implementation is exactly where that
is most likely and least visible. So a [`Proposal`] carries an oracle like everything else,
and one resting on `assumed` is merged by somebody willing to say where the behaviour came
from, or not at all.

That is a labelling requirement, not a prohibition, which is the whole design of the
`known_by` vocabulary: *an assumption that is written down can be counted, ranked, probed and
retired; one written as though it were a fact never will be.*

### What is reported, and separately

A measurement is settled by re-deriving it. A patch is settled by a person reading it. Both
in one list would let a diff inherit the trust the measurements earned, so `submit check`
prints them apart and says plainly that nothing here checked them:

```
1 source change(s) proposed - NOT checked by anything here:
  example.patch - ... [assumed, by a model]
    assumes: everything about it
  1 of them rest on nothing better than a guess.
```

`proposed_by` is a field rather than an inference. A patch written by a person and one
produced by a model need different reading, and a bundle that did not distinguish them would
make the careful reading the exception.

**Nothing generates these yet**, and that is now a gap rather than a policy. The shape is
settled, the promotion path is real, and what is missing is the generator - which can be
judged on what it produces instead of on whether it should exist.

