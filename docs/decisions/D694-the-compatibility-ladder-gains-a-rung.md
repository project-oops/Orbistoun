# D694 - The compatibility ladder gains a rung nothing can reach

**assumed** - 2026-09-15

`Reach::Presented` sits above `Reach::Flipped` and means the buffer a flip carried was read back
and held pixels the guest wrote. Nothing in the tree can award it. That is the decision, and the
unreachability is the point rather than a defect in it.

## Why a scale should be able to say "not yet"

Six guests were at the top rung with full standing, and none had produced a pixel. Nothing about
that was hidden - `Flipped`'s own documentation and COMPATIBILITY.md's prose both say a flip is a
place reached and not a picture shown (D558) - but the *table* said `flipped`, and a reader of a
table does not read the prose beside it. The frontier was advertising completion of a layer that
has produced no output.

A ladder whose top rung everything has reached carries no information about the work left. Adding
a rung above restores that: the corpus now ranks where it actually is, and the next stretch of
work becomes measurable in a way `flipped` could not express.

## Why no arm in `status_of`

Two ways to add a rung nothing reaches:

1. **A trace field, always false.** Rejected. A field nothing writes reads as machinery that
   exists, and a future reader has to go and find out that it never fires. It is plausible output
   in the shape principle 3 forbids - reporting more structure than the measurement supports.
2. **No arm at all, and a test that asserts the absence.** Taken. `status_of` simply has no branch
   producing `Presented`, and `the_top_rung_is_out_of_reach_until_a_buffer_can_be_read_back`
   asserts that over every input the function can be handed.

The second is honest about the state and *self-correcting*: it fails the moment someone wires a
presented buffer into the run path, and the failure message tells them to write the arm. The first
would have sat quietly forever.

## Why it passes D182's test

D182 refused a rung for reaching the time limit, on the grounds that *not dying is an outcome, not
a distance* - a guest spinning on four unimplemented functions survives, and ranking that above a
title that faulted after forty-seven imports put the least informative run in the corpus at the
top of the table.

Every rung since has had to be *a thing done*. Pixels are the strongest available form of that: a
buffer differing from what it held before the guest ran is a positive measurement against a known
prior, not the absence of an event. It is also the framebuffer-diffing oracle CLAUDE.md names as
the only cheap mechanical correctness signal in this codebase, so the rung is defined in terms of
the one graphics measurement the project already trusts.

## Why above `Flipped` and not replacing it

`Flipped` is a real distance and stays one. Reaching it requires opening an output, setting
attributes, registering buffers and configuring the port, each against a real implementation.
A guest that gets there and produces nothing has still done considerably more than one that
entered and faulted. Collapsing the two would lose that.

## What it costs

Nothing recorded changes. A rung nothing has reached is a rung no record claims, so every
`[status]` block and every comparison between runs is untouched; the ladder got longer at the top
rather than shifting under what was already measured.

## What would overturn this

A readback landing and the rung turning out to be the wrong granularity - most likely because
"pixels the guest wrote" splits further than expected, between a cleared buffer and a drawn one.
If that happens the fix is another rung or a distance *within* this one, not a retreat to
`flipped`.
