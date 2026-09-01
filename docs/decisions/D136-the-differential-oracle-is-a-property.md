# D136 - The differential oracle is a property, tested by generation


**Status:** assumed

`tests/agreement.rs` generates seeded programs from the instructions both fidelity levels
accept, runs each at both levels on a real device, and asserts identical registers and
identical memory.

D100 kept two wavefront models specifically so each could check the other, and until now
that oracle was used anecdotally - a handful of hand-written shaders with
`the_models_agree_about_…` in the name, each covering the instruction it was written for.
Used as a property it covers sequences nobody would think to write, which is where a
disagreement is most likely to hide.

**What it cannot find, stated because it matters.** Both models dispatch through the same
`model::instruction`, so an instruction translated wrongly *once* is translated wrongly in
both and they agree perfectly. This finds mistakes in what differs - the register files,
the masking, the lane loops - and is blind to what does not. `execute.rs` asserts what
instructions *do*, against values worked out by hand, and covers the other half. Neither
replaces the other.

**Verified by breaking a model.** Indexing the wavefront scalar file one register high -
a plausible-looking slip - is caught on the first seed. A property test that has only
ever passed is indistinguishable from one that asserts nothing, and this project has
found that failure in its own tests more than once.

**Bounded inputs on purpose.** Addresses are kept inside the memory window, because what
the two models do past the end is undefined rather than required to match; asserting
agreement there would be asserting about a coincidence. Instructions needing a lane mask
are left out, because the per-lane model refuses them and there would be nothing to
compare against.

The test insists most programs were actually compared. A generator emitting something
neither model accepts would otherwise pass while checking nothing.

