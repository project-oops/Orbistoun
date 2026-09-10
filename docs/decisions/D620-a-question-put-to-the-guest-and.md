# D620 - A question put to the guest, and the answer was no

**Status:** measured
**Date:** 2026-09-08

## What was worth asking

D615 left the wall at a precise place: the render thread blocks on `UnityFTMFlipQueue`, which has
no registrations because `sceAgcDriverAddEqEvent` is unimplemented, while the two queues
`sceVideoOutAddFlipEvent` does feed are never waited on. D613 predicted that implementing the
registration alone would not help, because `post_event` matches by identifier and the AGC call
carries none.

Before writing any of it, there is a narrower question a flip can answer on its own:

**If that wait completed, what would the guest do next?**

That is worth an experiment rather than an implementation. A guest that proceeds says a flip
completion is near enough what it was waiting for; one that wakes and immediately faults says
which field of a delivered event it read. Either is evidence and neither is a fix.

## The experiment

`ORBISTOUN_FLIP_TO_ALL`, a diagnostic that makes `sceVideoOutSubmitFlip` post its completion to
**every** queue rather than the registered ones. Off by default, declared as `Effect::Intervenes`,
so a verdict taken under it is recorded as measuring a settings change and not the emulator (D224,
D226, D227).

It guesses nothing about the registration. It only removes the routing, which is the one thing
standing between a flip and that queue.

## The answer

```text
baseline           UnityFTMFlipQueue (0 registered, 1 waits, 0 delivered)
FLIP_TO_ALL=1      UnityFTMFlipQueue (0 registered, 1 waits, 0 delivered)
                   verdict  same     nothing moved
```

**Zero delivered, to any queue, even with the routing removed.** Which can only mean one thing:
no flip is submitted after that queue exists.

The handles say the same. The two video-out queues are `0x5e2d0000b440` and `0x5e2d0000b460`;
`UnityFTMFlipQueue` is `0x5e2d0000ee40`, allocated much later. All forty-four
`sceVideoOutSubmitFlip` calls happen before the queue the guest goes on to block on was created.

## What that rules out

A whole line of attack. Implementing `sceAgcDriverAddEqEvent`, working out what identifier a post
would carry, and wiring a completion to the flip path would have been days of careful work
against a queue that **nothing was ever going to post to in this run anyway**, because the guest
stops submitting before it starts waiting.

The wall is not "orbistoun does not complete graphics work". It is that the guest reaches a state
where it waits for a frame it has not asked for. What drives the *next* flip is the question, and
it is a different question from the one four decisions have been circling.

## The diagnostic stays

It answered its question in one run and cost one flag. Kept, because the same question will be
worth re-asking the moment anything changes about when the guest submits - and because a
diagnostic that has been used once and recorded is worth more than one nobody has watched work.
