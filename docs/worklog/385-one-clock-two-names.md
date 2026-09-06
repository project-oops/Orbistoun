# 2026-09-04 - (/loop) One clock, two names, and one origin

```
suites 125   tests 2015   clippy/fmt/identity clean
guest unchanged: 197 distinct, ~418,900 calls
```

Twenty-second cron tick. `sceKernelClockGettime` implemented - the third vendor/POSIX pair after
`stat` (D525) and `pread` (D526), and the first where sharing was not optional.

## Why the shared part moved down a crate

The other two pairs live in one crate, so sharing the success path was a function call. This one
does not: POSIX `clock_gettime` is `orbistoun-libc`, the vendor name belongs to `libkernel` which
`orbistoun-kernel` declares, and **kernel does not depend on libc** - nor should it, they are
sibling subsystems.

So the shared part went to `orbistoun-hle`, which both already take: the clock identifiers, the
families that can be answered, and the monotonic origin. All three are facts about the
**platform** rather than about either library.

## The origin is the reason, not the line count

`since_start` measures from the first call **anywhere in the process**. While one library held
it that was true by accident; now it is by construction.

Not cosmetic: a guest reading a monotonic clock through both names must not get two different
elapsed times. Two copies of an `Instant::now()` `OnceLock` would have given exactly that, and
**nothing in either library's tests would have noticed** - each is right on its own.

## Failure differs, as it does for the other pairs

POSIX answers `-1`; a `sceKernel*` call answers `0x8002_00xx`. `EINVAL` for a clock with no
honest source. **Which errno the console answers is unmeasured**, so the test pins the family and
the sign - what a caller branches on - not a code no run has established.

Both halves broken and watched to fail: the unwritten second field, and answering `-1`.

## Refusing rather than answering the nearest thing

The unanswerable families are refused, inherited from the POSIX side: a guest measuring its own
CPU time and receiving wall time gets a number that **looks right and is not**.

## What the tests cannot prove

The values - a wall clock and an elapsed time are not reproducible. What is checked is *which
source* answered: the monotonic reading is small because the process just started, the real-time
one is past a date already in the past. Said in both tests rather than left to be inferred.

Decision: [D536](../decisions/D536-one-clock-two-names-and-one-origin.md).
