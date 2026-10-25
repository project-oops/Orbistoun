# D972 - A program's control flow is laid out as structure where it nests

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A program's blocks are emitted as the selections and loops they form, rather than as arms of one
switch on a program counter inside one loop (D098). The dispatch loop stays as the fallback for a
segment whose flow does not nest, and as the reference structure is checked against.

- **Selections.** A conditional branch is a selection that merges at the branch's immediate
  post-dominator, computed over the blocks with each loop's back edges sent through a node that
  stands for the whole loop. Where the two arms meet earlier than that, at a join, and one arm
  skips past the join to the post-dominator, the selection merges at the join. The skip writes its
  target into the program counter and leaves through the selection merges it is inside. What lies
  between the join and the target runs under a test of the counter.
- **Loops.** A natural loop is a loop with its own merge and continue target. A back edge is a
  continue, and an edge out of the loop is a break.
- **Routed exits.** A break that cannot be expressed directly writes where it goes into the
  counter, and the loop's merge routes on the counter. This covers the program's end from inside a
  loop, a continue or break of an outer loop, and a loop that leaves for two blocks. A routed loop
  clears the counter on entry, so a value from an earlier pass routes nothing.
- **Leaving early.** A jump to the program's end from inside a selection breaks out of a loop that
  runs once around the segment.
- **Waves that ended.** A wave that ended holds the exit value in the counter, as in the dispatch
  loop, so a later segment of a group's program (D892) skips it.
- **The fallback.** A segment whose flow is irreducible, or whose blocks a jump would place twice,
  keeps the dispatch loop. A later segment after it is entered as the loop expects.
- **Unreachable blocks** are not emitted.
- **Placed once.** Every block the entry reaches is placed exactly once, or the plan is refused
  before anything is emitted. A structured module therefore has the guest's edges, drawn as
  structure.

`ORBISTOUN_CONTROL_FLOW=dispatch` lays out every program as the dispatch loop, to compare the two.

**Why:** the dispatch loop gives the host compiler one loop in which any block may follow any
other. Grand Theft Auto V's program at `0x74005b7e8600` (2,171 instructions, 210 blocks) makes a
643,528-word module that an RTX 5070 Ti's driver compiles in 256 s. Laid out as structure it is
642,410 words and compiles in 198-204 s, with the same lane work (D956) in both. Its first layout
was refused because an arm skipped past its join, which the escapes above now express.

**Rejected:**
- Splitting a block reached from two places into copies: the code grows with each copy, and the
  driver's time grows with the code.
- One flag per block for every program: as general as the dispatch loop, and as opaque to the
  driver. It is kept for flow that does not nest, as the dispatch loop.
- A `return` for the program's end: the epilogue after the body publishes the dispatch's
  observation, and it must run.
