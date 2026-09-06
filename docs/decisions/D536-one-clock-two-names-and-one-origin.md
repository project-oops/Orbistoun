# D536 - One clock, two names, and one origin

**guest-observed** - 2026-09-04

`sceKernelClockGettime` is implemented. It is the third vendor/POSIX pair after `stat` (D525)
and `pread` (D526), and it is the first where sharing the implementation was not optional.

## Why the shared part moved down a crate

The other two pairs live in one crate: `orbistoun-fs` owns both `stat` and `sceKernelStat`, so
sharing the success path was a function call. This one does not. POSIX `clock_gettime` is
`orbistoun-libc`; `sceKernelClockGettime` belongs to `libkernel`, which `orbistoun-kernel`
declares - and **kernel does not depend on libc**, nor should it. They are sibling subsystems.

So the shared part went to `orbistoun-hle`, which both already take: the clock identifiers, the
families that can be answered, and the monotonic origin. All three are facts about the
*platform* rather than about either library, which is what makes that the right floor for them.

Copying thirty lines into the second crate would have compiled and would have drifted.

## The origin is the reason it matters, not the line count

`since_start` measures from the first call **anywhere in the process**. While one library held
it that was true by accident; now it is true by construction.

It is not cosmetic: a guest reading a monotonic clock through `clock_gettime` and again through
`sceKernelClockGettime` must not get two different elapsed times. Two copies of an
`Instant::now()` `OnceLock` would have given exactly that, and nothing in either library's tests
would have noticed - each is right on its own.

## Failure differs, as it does for the other two pairs

POSIX answers `-1`; a `sceKernel*` call answers `0x8002_00xx`. `EINVAL` for a clock this has no
honest source for - the per-process and per-thread CPU clocks, `CLOCK_UPTIME` - because that is
what the refusal *is*.

**Which errno the console answers is unmeasured**, so the test pins the family and the sign,
which is what a caller branches on, rather than a code no run has established.

Both halves were broken and watched to fail: the unwritten second field, and answering `-1`.

## Refusing rather than answering the nearest thing

The families that cannot be answered are refused, and that is the older decision this inherits:
a guest measuring its own CPU time and receiving wall time gets a number that **looks right and
is not**. Answering the nearest available clock is how that happens, and it is the failure
principle 3 exists to prevent.

## What the tests cannot prove

The values. A wall clock and an elapsed time are not reproducible, so what is checked is *which
source* answered - the monotonic reading is small because the process just started, the
real-time one is past a date already in the past. That distinguishes them without pinning either
to a number no run can repeat, and it is said in both tests rather than left to be inferred.
