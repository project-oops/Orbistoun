# D583 - The loop checks that a run repeats before it believes any comparison

**Status:** measured
**Date:** 2026-09-08

## Every step the dispatcher takes is a comparison, and none of them checked

`orbistoun-turn` maps each finding to a step and runs the mechanical ones: plant a sentinel and
see whether the fault moves, force a return and see whether the guest gets further, poison a
region and see what changes. **Every one of those is a difference between two runs.**

All of them assumed two runs of one build behave identically, which D181 and D238 require - and
which nothing anywhere had ever checked. A guest that varies on its own puts its own variation
into the difference, so the sweep reports the noise as the effect of the intervention.

That is principle 3's *"an intervention that moves a wall is not a diagnosis"*, arriving one
level below where the caveat is printed: `orbistoun-env` marks which diagnostics intervene and
the run report says so, and none of that helps when the *baseline* is the thing that moved.

## It was not hypothetical, and the first run of the check proved it

`Step::CheckRepeats` runs the guest twice with nothing applied and compares the two signals every
other step compares - where it died and how far it got. On PPSA03416 it says:

```text
*** two runs of this build DISAGREE - 192 imports/0xa0 against 193 imports/0xa0;
    every comparison below measures that as well as its own intervention
```

and four lines further down, in the same turn, unchanged from before:

```text
*** libc::memcpy answered the code the guest followed; zero reaches 193 against 192, faulting at 0xa0
```

**That starred finding is the disagreement.** 192 against 193 is exactly what the guest does on
its own; the dispatcher had attributed it to forcing `memcpy` to zero, and would have gone on
doing so. One check, one boot pair, and a result the tool had been presenting as a discovery is
withdrawn.

## Two runs, and first

**Two rather than ten.** One disagreement is enough to know; booting ten times to raise
confidence in a *negative* costs ten boots to learn nothing that changes what anyone does.

**Ahead of the findings rather than ranked among them.** It is a precondition of reading the
turn, not a lead of its own - a plan that ranked it would put it below whatever the report
thought was worse, and everything above it would be read before the line saying it could not be.
So it heads every plan, including one with no findings at all.

**Compared on the signals the other steps use.** A step that called two runs different on a
signal nothing else reads would refuse turns for a variation none of them could have noticed.
Where it died and how far it got are what `experiment::Outcome` already carries and what every
sweep already compares.

## The sentence says what it costs

`DoesNotRepeat` prints the consequence, not only the fact: *"every comparison below measures that
as well as its own intervention"*. A reader told that two runs differed, and not that it makes
the lines below unreadable, will read the lines below. The test asserts the sentence contains
that clause, which is a strange-looking assertion and the reason the line exists.

Watched failing: comparing only the fault address lets the live case through, because both runs
fault at `0xa0` and differ only in reach.

## What this does not establish

**That a repeating run is a correct one.** Two runs agreeing means a comparison between them is
readable, and nothing more. A build that is deterministically wrong passes this.

**Nor that the two signals are enough.** They are the two the dispatcher compares, so a run that
repeats on them and varies elsewhere would pass here and still poison a step that read something
else - and D581's mapping record already shows one: two runs can agree on fault and reach while
their arena addresses differ. This checks what the dispatcher uses, not everything that varies.

**Nor does it fix anything.** It refuses to let a turn be misread. D582 is what made PPSA03416
repeatable up to its first thread, and after that thread this check will keep saying no - which
is the correct answer rather than a limitation of the check.
