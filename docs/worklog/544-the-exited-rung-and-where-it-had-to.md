# 544. The `Exited` rung, and where it had to sit

**2026-09-14** - the ladder gains a rung; the placement was the whole job

## What landed

`Reach` is now `Rejected -> Parsed -> Linked -> Entered -> Exited -> Flipped`. A guest that leaves
by calling `exit` is recorded as having got further than one that merely ran. Reasoning in D685.

## The placement, which took longer than the code

The obvious reading of "add a rung for a guest that exited deliberately" is a rung at the top -
finishing is the furthest a program can get. `Status::ranking_key` makes that wrong:

```rust
(self.reach, self.imports, self.answered(), self.standing, self.frames, self.calls)
```

The rung is compared **first**, so it dominates every quantity beneath it. A program whose first
instruction is `exit(0)` would then outrank a title rendering frames. That is not hypothetical: on
this corpus it moves `obscene-payload` from **fifth to first** on the frontier, putting the
conformance probe at the head of the "closest to running" list, above `PPSA99980`, `PPSA03416` and
`PPSA02664`.

The codebase had already been bitten by this twice and said so. D182 refused a rung for surviving to
the time limit; D558 records *"the exact failure that cost `Entered` its rung above 'survived'"*.
Reading those before writing the variant is what turned a one-line change into the right one.

Checking it costs nothing and settles it: the frontier and the record are both sorted by
`ranking_key`, so the ordering could be read off rather than argued about.

## The test that is the decision

`a_trivial_exit_does_not_outrank_a_title_that_rendered` asserts the ordering D685 turns on. Moving
the variant above `Flipped` fails it and `a_frame_outranks_a_deliberate_exit`, with the message
naming the case. Verified by doing it, then reverting.

`giving_up_does_not_earn_the_rung` pins the other half: `abort`, an unhandled signal and the time
limit all stay at `Entered`. Stopping is not finishing, and the three are different words for three
different things - `StopReason` already had them separated, which made the distinction free.

## Honest about what it does not do

**It does not fix the case that prompted it.** `obscene-payload` flips *and* exits, so it stays at
`Flipped` and the `BACK` verdict from worklog 543 is unchanged: its import count really did fall by
one when it stopped running past its own `exit`. At equal reach the record still has no way to say
"better in a way the score does not capture". That is a change to what `beats` compares, and it wants
its own evidence rather than being folded in here.

So the rung is worth having for guests that finish without ever presenting - and the motivating
regression stays open. Both are true and it would be easy to report only the first.

## Surprises

**A doc comment can be orphaned by an insertion and only clippy notices.** Placing the
`DELIBERATE_EXIT` constant by matching on the `pub fn` line put it *between* `status_of` and its
documentation, silently reassigning nine lines of prose to a `const`. `cargo build` was happy;
`missing documentation for a function` under `-D warnings` caught it. Anchor on the start of a doc
block, not on the item it documents.

**One workspace test fails and it is not this work.** `orbistoun-gpu-vulkan::dispatch` reports a
shader address that resolves to no mapped memory. That crate is unmodified and the test dates from
August; the failure is in register decode and memory mapping - `registers.rs`, `pipeline.rs` and
`packets.toml`, all carrying another session's uncommitted changes. Recorded rather than touched.
