# Control flow: a switch inside a loop


Guest branches translate. Forward branches skip blocks, backward branches loop, and both
run on a real device. Forty-one execution tests.

The shape is a dispatch loop (D110): every guest basic block is an arm of one `OpSwitch`
inside one loop, a program counter selects the arm, and a branch is an assignment to that
counter. No control-flow analysis, no relooper, no reducibility requirement - the guest
can branch backwards into the middle of anything from several places at once and the
result is still valid SPIR-V.

**It was mechanical because the architecture was already built for it.** `predicated.rs`
has said since it was first written that registers live in memory because "the dispatch
loop this strategy is heading towards puts every guest block behind a different arm of a
switch. Values cannot cross those arms." That note was written months of work before
anything needed it, and it is the reason this was a day's work rather than a rewrite. I
had described this in conversation as an unsolved design problem; it was a solved design
problem with the solution written into a module doc comment.

**The verification that mattered was the existing suite.** Thirty-eight tests, every one
of them a straight-line shader, all now running through a switch-in-a-loop. If the
scaffolding were wrong they would fail, and they are the tests with the most coverage of
anything else the translator does. `spirv-val` confirmed the structure separately.

**A conditional branch adds no blocks.** The arm evaluates the condition and `OpSelect`s
between two counter values, so every arm has exactly one predecessor and one successor.
Nesting a selection per conditional would have reintroduced the structure the whole shape
exists to avoid.

**Surprises.**

- **The block-splitting test caught my own arithmetic, not the code's.** A branch offset
  counts dwords from the instruction *after* the branch, and I wrote a test expecting the
  target to be four bytes further along than it is. The code was right. That error, made
  the other way round, would land on a real instruction boundary most of the time - so
  nothing downstream would complain and every branch in every shader would go one
  instruction wrong.

- **`touches_mask` was about to match an opcode range across every family.** Opcodes 6
  to 9 are the mask branches in SOPP and are ordinary arithmetic everywhere else, so
  without the family check any shader containing a `v_mul_f32` would have been routed to
  a model sixty-four times slower. Caught while writing it rather than after, but only
  because the compiler rejected the call - the version with the bug never reached disk,
  which is luck rather than process.

- **`OpSwitch`'s tail is literal-then-label**, not a run of identifiers. The builder's
  identifier check would have reported every case value as an undefined identifier and
  rejected a module that was fine, so the shape table gained a stride. A check that
  reports false failures is worse than no check, and this one was one entry away from
  being that.

- **The identifier check had to be narrowed for function bodies.** Forward references are
  legal and necessary there: a branch names a label that appears later, a loop header
  names its own merge block. The half that caught the original driver fault still applies
  everywhere, and the declarations section - where that bug lived - keeps both halves.
  Recorded as D111 rather than quietly relaxed.

- **`orbistoun-abi` failed a workspace run again**, and passed alone. Second occurrence,
  already in the backlog as fixed-address test contention, and it has now cost a gate run
  twice. The backlog entry predicted it would be "blamed on flakiness until it costs
  someone an afternoon" - it is at about half an afternoon.

**Not done.** Branches on the scalar condition code are refused, because nothing sets it:
the scalar compares (`SOPC`) do not translate, so it would read zero in every shader and
every such branch would take the same path. That is the next obvious piece, and it is
what the `control` fixture needs to translate end to end. `Fidelity::Subgroup` is still a
stub. The single-block case still pays for the loop, by choice.

