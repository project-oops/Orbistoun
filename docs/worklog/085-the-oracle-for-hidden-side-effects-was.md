# The oracle for hidden side effects was in the fixtures already


D129 said nothing here could observe whether an instruction writes the condition code -
it is invisible in the encoding, the operand layout, and any test that checks
destinations. That was wrong, and the counter-example had been sitting in `control.txt`
since it was generated:

    s_cmp_lt_i32   <- sets it
    s_mov_b64      <- between
    s_cbranch_scc1 <- branches on it

A compiler putting an instruction there is asserting it does not write the condition code,
or the shader it emitted would be wrong. `the_corpus_agrees_about_hidden_side_effects`
mines every fixture for those windows and checks them against the translator. One window,
one instruction confirmed, and it grows with the corpus.

246 tests green across the shader-side crates.

### Surprises

- **"No oracle exists" was a claim about attention, not about the world.** The evidence was
  in a file this project generated, in a test directory it reads on every run. What was
  missing was the question.
- **A list is not automatically the D122 mistake.** That one duplicated derivable data. This
  one encodes a fact with a single source and no derivation - the fault there was the
  duplication, not the listing, and the fix here is a check rather than a deletion.

