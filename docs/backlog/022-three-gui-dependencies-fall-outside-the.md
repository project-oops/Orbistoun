# Three GUI dependencies fall outside the licence allow list


`cargo-deny` reports `licenses FAILED` on every run, and `./bin/orbistoun check` treats it
as advisory rather than blocking - so the tree is green while the policy is unmet, which is
the shape this project generally refuses.

The three are `clipboard-win` and `error-code` (**BSL-1.0**, the Boost licence - permissive,
and arguably an omission from the list) and `epaint_default_fonts` (**OFL-1.1** and
**LicenseRef-UFL-1.0**, font licences, which are a different question from code licences and
were not considered when the list was written). All three arrive through `eframe`, so they
are `orbistoun-gui`'s cost and nothing else's.

Three ways out, and the choice is a decision rather than a chore: widen the allow list with
the reasoning written down, carry per-crate exceptions, or drop the dependency. What should
not continue is a check that fails on every run and is ignored on every run - that is how a
guard becomes decoration (D199).

## Correctness

