# 547. Records update on not-worse-and-different, and three wrong diagnoses

**2026-09-14** - the rule changed; the case that prompted it was something else entirely

## What landed

`keep_status` now gates on `Status::worth_recording` rather than `Status::beats`: comparable, not
below on the ranked key, and differing in the key or in the outcome. `beats` is unchanged and keeps
its meaning. Reasoning in D687.

The case it buys: a guest that reaches exactly as far but dies at a **different address** now updates
its record, where before the file went on naming a fault site the guest no longer reaches.

## Three wrong diagnoses before the right one

The payload's record has said `outcome = "0x5e2d"` since 2026-09-10 while every recent run stops
deliberately. Changing the recording rule was supposed to fix that. It does not, and getting to why
took three wrong turns:

1. **"`beats` is too strict."** It is not. The run is genuinely worse.
2. **"The run never reaches the recording step."** It does. `record_compat` ran both times.
3. **"The stop reason is not persisted."** It is - `"stopped": "the guest called exit"` is in the
   trace. My grep pattern required no space after the colon and found nothing, and I read that
   absence as a fact about the program.

The actual answer: the payload's recent runs produce **zero frames** and reach `Exited`; the stored
record has **eight frames** and reaches `Flipped`. A lower rung is worse, and the record is right to
keep the better one.

## Why it took three tries, which is the reusable part

On the `NotBetter` path the CLI prints **nothing at all** unless the run is below best. So a run that
was compared and refused looks exactly like a run that was never compared - and every diagnosis above
is a different guess at which of those happened. The silence is the bug that cost the time, not the
rule.

`orbistoun-cli compat record <path>` is what settled it, by printing the refusal the run had
swallowed. **When a run says nothing about its own record, ask the record.**

## The finding underneath, not chased

**The payload used to flip and no longer does** - eight frames on 2026-09-10, zero now - and the
retained record hides it, because keeping the better entry is exactly what it is for. A regression
that is invisible *because the record worked correctly* is worth knowing about.

## A message that was false for three situations

The refusal said *"the status entry is better or equal"* whether the entry was better, equal, or **not
comparable at all** - a different stub policy short-circuits `beats` before a single number is read.
Now "better, or the same run again"; the incomparable case is still conflated and is named in D687 so
the next person knows.

## Surprises

**A wrong grep became a claim about the program.** `"stopped":"[^"]*"` does not match
`"stopped": "..."`. I reported the stop reason as unpersisted on that basis, which would have sent
the next change at plumbing that already works. Second time today a search pattern produced a
confident wrong answer about absence - the first was `OBS|bytes` in worklog 534. **An empty grep is
evidence about the pattern before it is evidence about the code.**
