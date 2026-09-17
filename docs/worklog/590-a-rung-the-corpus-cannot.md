# 590. A rung the corpus cannot reach

**2026-09-15** - the compatibility scale can now say "no pixels yet", which it could not before

## The problem, as the scale had it

`flipped` was the top rung. Six guests sit on it with 100% standing - three commercial titles at
one frame each and three conformance probes at 8 to 53 - and **not one has produced a pixel**.

The rung was never dishonest about this. Its own documentation says so plainly: *"It does not mean
a picture was displayed. Nothing scans a buffer out here, and a flip completes the instant it is
accepted."* COMPATIBILITY.md's prose says it too, since D558: *"a place reached and not a picture
shown"*.

**The table did not say it, and the table is what gets read.** A ladder whose top rung everything
has already reached measures nothing about the work remaining, and this one was reporting six
guests as finished with a layer that has produced no output at all.

## What was added

`Reach::Presented`, above `Flipped`. It claims that the buffer a flip carried was read back and
found to hold pixels the guest wrote.

Every guest in the corpus now ranks below the top rung. That is the accurate reading and it was
not previously expressible.

## Nothing awards it, deliberately

`status_of` has **no arm** for the new rung, because there is nothing to read back: no renderer is
attached to the run path, so no presented buffer exists to inspect. The alternative - adding a
trace field nothing writes - was rejected: a field that is always false reads as implemented
machinery and is exactly the plausible output principle 3 forbids.

So the absence is asserted instead, beside the function that decides rungs:
`the_top_rung_is_out_of_reach_until_a_buffer_can_be_read_back` runs every `reached` word
`status_of` matches on, crossed with three frame counts and a deliberate exit, and requires the
result to be below `Presented`. The claim is about the function, not about an example.

**The day that test fails is the day the readback landed**, and its message says so: *"if a
presented buffer can now be read back, give `status_of` its arm and replace this test with one
that exercises it"*. Made to fail before it was believed - pointing `"Entered" if frames > 0` at
`Presented` produces exactly that message.

## Why this is a rung and not an outcome

D182 refused a rung for surviving to the time limit, because *not dying is an outcome, not a
distance*. Every rung since has had to pass that test, and pixels pass it in the strongest form
available here: a buffer differing from what it held before the guest ran is a positive
measurement against a known prior, and it is the framebuffer-diffing oracle CLAUDE.md already
calls the only cheap mechanical correctness signal in the codebase. There is no way to spin into
it.

Recorded as D694.

## What it does not fix

The other half of the same problem is untouched: a title that stops and waits for a button press
records `ran to the time limit`, and so does a conformance probe that completed cleanly. Those are
different events sharing a label, which is the same defect one level down, and the same shape as
the `exited` rung added earlier for guests that stopped deliberately. Next.

## Surprise

**Adding a rung above everything changed no record.** The instinct was that re-ranking the corpus
would invalidate recorded bests. It cannot: a rung nothing has reached is a rung no record claims,
so every `[status]` block is untouched and every comparison between two runs is unaffected. The
scale got longer at the top rather than shifting underneath what was already measured - which is
the cheap direction to extend a ladder, and worth knowing before the next rung is proposed.
