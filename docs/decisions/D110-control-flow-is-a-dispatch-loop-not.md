# D110 - Control flow is a dispatch loop, not reconstructed structure


**Status:** decided (2026-08-21) - and the deferral now has a number behind it

Every guest basic block becomes an arm of one `OpSwitch` inside one loop, selected by a
program counter held in a private variable. A branch is an assignment to that counter.

SPIR-V requires structured control flow; the guest has none. Recovering structure from a
flat stream of signed offsets is a real compiler problem, and where the guest's flow is
irreducible there is no structure to recover - only a transformation that invents one.
This shape needs no analysis at all and is valid however tangled the flow is: backwards,
into the middle of anything, from several places at once.

**It was the plan from the beginning.** `predicated.rs` has said since it was written
that the register file lives in memory because "the dispatch loop this strategy is
heading towards puts every guest block behind a different arm of a switch. Values cannot
cross those arms." That decision was made for this, and it is why this was mostly
mechanical rather than a rewrite.

**A conditional branch adds no blocks.** The arm computes the condition and `OpSelect`s
between two counter values, so every arm has one predecessor and one successor and the
switch stays flat. The alternative - a nested selection per conditional branch - would
reintroduce exactly the structure this avoids.

**The loop is emitted even for a single-block shader**, where it is pure overhead.
Collapsing that case would be easy and is deliberately not done: the single-block path is
the one every existing test exercises, so a second code path for it would be the
under-tested one. Every one of those tests now runs through the loop, which is what
verified the shape.

### Confirmed, on a different argument than the one written here

The reason above has partly expired. Eight named tests build multi-block shaders now,
plus a compiled fixture with real control flow, so the loop is no longer exercised only
incidentally by single-block ones. "A second path would be the under-tested one" is a
weaker claim than it was.

It stays anyway, and the reason is arithmetic rather than judgement. This entry deferred
the question "until there is something to measure", and nothing measured it - which is how
a deferral quietly becomes permanent. So it is measured now:

| | words |
|---|---|
| an empty module | 591 |
| one instruction | 599 |
| **what the instruction added** | **8** |

**The loop is not where the cost is.** A module's fixed preamble - the types, two register
files, two storage buffers - is seventy times what an instruction costs. Collapsing the
loop would save a fraction of one percent of a small module and buy a second emission path
to keep correct, which a retarget has already shown the price of.

If anyone does want a smaller module, those numbers say where to look, and it is not here.

The test that produced them stays, and its assertion is **structural** rather than by size:
a collapsed loop would barely move the total, so a size check would not notice it. It looks
for the loop and the switch directly, and fails if a single-block shader stops going
through them - which is exactly the change this entry says needs a decision first.

**A branch target that is not an instruction boundary is refused**, not rounded to a
nearby one. An offset can land inside an instruction, and moving it would produce a
program that runs and is not the one the guest wrote.

**Branches on the scalar condition code are refused.** Nothing sets it - the scalar
compares are not translated - so it would read zero in every shader and every such branch
would take the same path every time. A shader that always takes one side of every `if`
runs and produces output.

