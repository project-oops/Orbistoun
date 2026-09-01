# Lanes that do different things


A translated shader can now diverge. `v_mbcnt_lo_u32_b32` and `v_mbcnt_hi_u32_b32` let a
lane learn its own index, `v_lshlrev_b32` and `v_add_u32` turn that into an address, and
`v_cmp_lt_u32` compares it - so lane *n* computes its own value, tests its own condition,
and stores to its own address. Thirty-eight execution tests on a real device.

That closes a hole that had been open under everything built this session. Every mask
test until now compared the same two registers in every lane, so every mask was all-ones
or all-zero - and a translation that tested the mask against zero rather than reading it
bit by bit would have passed all of them. The new test narrows the mask to lanes 0-3 out
of 64 and checks all sixteen observable words: four written with their own index, twelve
untouched.

**A lane is not told which lane it is.** There is no lane-id instruction and no value
handed to the shader. `v_mbcnt` counts the set bits of a mask *below* this lane, so with
an all-ones mask the count is the index - and getting a full index takes both halves in
sequence, the high one adding the low one's result. The boundary is the part to be careful
with: *strictly* below. Including the lane's own bit shifts every index by one wherever
that lane is active and leaves it right wherever it is not, so a test with a full mask
catches it and a test with an empty mask does not.

**Surprises.**

- **`v_lshlrev_b32` takes its shift first and its value second.** The "rev" in the name.
  Read in written order it computes `2 << index` where `index << 2` was meant - and those
  are equal for lane 2. So a test on any single lane can pass a wrong translation, and
  what pins it is asserting every word of the window rather than a sample.

- **A test was written and then deleted for adding nothing.** A separate case for the
  shift asserted strictly less than the test above it already did. Keeping it would have
  been free and would have quietly inflated what the suite appears to cover; the property
  it was reaching for is now a comment on the test that actually proves it.

- **VOP3 opcodes are ten bits, so `v_mbcnt` is 652, not 140.** Written from the solved
  table rather than inferred from where the mnemonic sits among its neighbours - the
  first attempt guessed a three-digit-family number and produced an arm that never fired,
  which is exactly the failure mode that looks like an unimplemented instruction.

**Not done.** Branching, still. Everything above is predicated execution: the mask decides
who acts and control flow never leaves the straight line. A jump taken when no lane
survives cannot be expressed, which remains the substance of D098 and a design problem
rather than an implementation one. `Fidelity::Subgroup` is still a stub. The lane and
wavefront models can no longer be compared on any shader that diverges, because the lane
model refuses them - which is correct and does cost the differential oracle its reach
exactly where divergence begins.

