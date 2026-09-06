# D567 - A placeholder that names its own source

**Status:** measured
**Date:** 2026-09-04

## The problem, stated by the code itself

Every unimplemented function answers the same `0x7fff_0001`. So when one turns up in a guest's
argument, the report knows *some* unimplemented function produced it and never which - and
`error_used_as_pointer`'s action says so out loud:

> find what answered with that code just before

That is a person's search, and D299 already ruled on it: **a finding whose action sends a reader
looking must carry what they are to look at.** The finding was breaking a rule this repository had
written down, in the one place the rule was hardest to keep.

It cost real memory to not have. A work-area sizer answered the placeholder and PPSA28061 handed
it to `malloc` **twice** - four gigabytes - and all the report could offer was the three calls
before it (D564).

## The decision

An opt-in diagnostic, `ORBISTOUN_TAG_PLACEHOLDERS`. Under it a stub answers
`0x7fff_0000 | (0x10 + its stub slot)`, so **the value is the attribution**.

Measured working - each unimplemented function now answers a distinct code:

```text
sceCommonDialogInitialize                 -> 0x7fff0091
sceAgcDriverRegisterDefaultOwner          -> 0x7fff021a
sceAgcCreateShader                        -> 0x7fff0225
libkernel::0x04df812afad225d7             -> 0x7fffbeac
```

The trace already indexes every call by the stub it landed on, and that is the same numbering the
tag carries, so a value resolves to a name with **no new plumbing**.

## Why opt-in rather than the new default

**Sixty-seven documents cite `0x7fff0001`**, and several decisions rest on seeing it: D516, D523,
D524 and D564 all fix an arity by spotting orbistoun's own placeholder left in a register. Changing
the value everywhere would make all of that prose stale at a stroke - the precise failure this
project spends most of its discipline avoiding.

Opt-in costs nothing and invalidates nothing. An ordinary run answers `0x7fff_0001` and every
recorded fact stays true; a run that wants attribution asks for it.

It is `Effect::Intervenes`, because it changes what the guest is told - a guest branching on the
exact value takes a different branch, and a verdict under it measures a settings change.

## The floor, and why it is 0x10

Tags start at `0x7fff_0010` because `0x7fff_0000..0x7fff_0010` is the fixed range the `GuestError`
codes occupy. A tag must never be mistaken for one of those, and - the direction that actually
bites - **a fixed code must never be read as a tag**: attributing `0x7fff_0001` to whichever import
sits at slot 0 would be a confident, wrong answer, which is worse than the vague one it replaced.
Guarded, and watched failing with the floor removed.

## An override still wins

Precedence is `overridden.or(tagged).or(declared)`. A person who set `ORBISTOUN_RETURN` for a
function gets what they asked for, tagging on or not. Found by testing: the first attempt to
demonstrate the 4 GiB bug under tagging forced the sizer's return *and* enabled tagging, and the
override quietly beat the tag. The precedence is right; the test was wrong.

## What this does not establish

**That the tag names the right function.** The service tags with a global stub index and the trace
records the same one, which is true today and is not asserted anywhere - nothing would notice if
one of them started counting differently, and the symptom would be a confident finding naming the
wrong import. The guard says so.

**Nor that it would have caught the four gigabytes.** That is the case it was built for and it can
no longer be run: the sizers are implemented, so no placeholder reaches `malloc` any more. The
mechanism is proved by unit test and by the tagged returns above, not by the bug it was too late
for.
