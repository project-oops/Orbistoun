# D614 - The timeout that arrived early

**Status:** measured
**Date:** 2026-09-08

## Three tests that flaked all session

`a_timed_acquisition_gives_up_at_its_deadline`, `a_timed_wait_never_gives_up_before_its_deadline`
and `a_timed_semaphore_take_gives_up_and_can_be_rescued` failed intermittently under the gate and
passed on their own, every time. The obvious reading is a test that is too tight, and the obvious
action is to loosen it.

The message says otherwise:

```text
and it did not give up early: 79.8797ms
```

A deadline of eighty milliseconds, and the wait came back after 79.88. **The test is right and
the primitive is wrong.**

## Where the fraction of a millisecond comes from

```rust
let remaining = deadline.saturating_duration_since(Instant::now());
let (next, outcome) = signal.wait_timeout(guard, remaining).ok()?;
if outcome.timed_out() && blocked(&guard) {
    return None;
}
```

`WaitTimeoutResult::timed_out` is the platform's own verdict, measured on the platform's own
timer. On this host that timer is coarser than `Instant`, so it can report a timeout a fraction
of a millisecond before the deadline `Instant` measures. Under load - which is what a full
workspace run is - the coalescing that makes it early is more likely, which is exactly why the
failures clustered in the gate and never in isolation.

So `sceKernelWaitSema` with a ten-millisecond timeout could answer `ETIMEDOUT` after 9.9
milliseconds. Small, real, and the kind of wrong answer a caller computing from it accumulates -
a frame budget derived from a clock that runs slightly fast is a frame budget that is slightly
wrong every frame.

## The change

Ask the clock rather than the flag:

```rust
if outcome.timed_out() && blocked(&guard) && Instant::now() >= deadline {
```

An early wake now goes round the loop again with a freshly computed `remaining`, which is what
the spurious-wakeup contract already required and what the `while blocked(...)` header already
implements for every other case. One condition; the shape was already there.

Five consecutive runs of the sync suite, and a full workspace run, with no failure.

## What this was nearly

A loosened assertion. `assert!(waited >= BRIEF - SLACK)` would have made the gate green, buried a
guest-visible defect under a tolerance, and left the next person to find it reading a test that
says the deadline is approximate when it is not. The flake **was** the finding, and the three
tests that kept failing were doing their job for a session and a half before anybody read the
number they printed.
