# 409. The pthread family, and a prediction kept

**2026-09-04** - directed, while the Agc probe set is out for hardware

## What was done

Implemented the pthread scheduling and identity family: `scePthreadGetschedparam` (34 calls),
`scePthreadSetschedparam`, `scePthreadSetprio`, `scePthreadRename`, `scePthreadCondattrDestroy`
and `pthread_setcancelstate`.

| | before | after |
|---|--:|--:|
| unimplemented functions called | 35 | **26** |
| calls landing on a stub | 914 | **32** |

`ThreadRecord` gained a scheduling policy and a cancellation state beside the priority it already
held - which is precisely what **D523 predicted would be needed**, in as many words: *"orbistoun
keeps no per-thread scheduling record ... the moment something does, this needs the per-thread
record rather than a wider `Ok`."* `scePthreadGetschedparam` is a read-back and it is called 34
times, so the moment arrived.

## The four-byte rule, somewhere unmissable

The guest passes `policy` at `0x…c86c` and `param` at `0x…c868` - **four bytes apart**, because a
`sched_param` is one `int` and a caller putting both on its stack puts them adjacent. An
eight-byte write to either destroys the other silently. D272 said this; here it is with no room
to be careless in. Guarded with a sentinel either side.

## What was deliberately not implemented

`scePthreadAttrGetstackaddr`. Its `arg0` and `arg1` in the run are **byte-for-byte identical to
`scePthreadAttrGet`'s**, and `arg0` is a *thread* handle where an attribute object belongs.
Nothing explains that, and D527 is this project mistaking a register that survived a tail-jump for
an argument. One unexplained coincidence is not evidence.

## Guards

Seven written, six broken and caught: eight-byte writes to the adjacent out-parameters; policy and
priority written to swapped slots; the priority read from the wrong register; `setprio` resetting
the policy; `set_scheduling` overwriting a policy it was told to leave; and a get writing before
checking the handle.

**One break did not fire, and that is written into the test.** Removing the `is_issued` preamble
from `pthread_rename` does not fail the handle guard, because `thread::rename` refuses unknown
handles too - the property survives on a second check downstream. A break removing both does fire.
It is recorded in the test because a guard that cannot say which of two checks is holding it up
would not notice one of them rotting, and the two do not cover the same set: `is_issued` catches a
handle handed out whose record never landed, and nothing else would see that.

## The problem this exposes, which is not in the work

**None of it moves a recorded metric.** `standing` is an integer percentage of calls reaching an
implementation, and 914 stubbed of 419,091 already rounded to 100%; so does 32. Reach, imports and
frames are unchanged, and the compat record cannot see a 96% reduction in calls resting on
placeholders. That is a gap in what the record measures. Left as an observation rather than
folded into this change - a metric edited in the same commit as the work it scores is a metric
nobody can check.

## Where it leaves the non-Agc surface

Nearly done. What is left outside Agc is `sceKernelUuidCreate` (3 calls), `_sigprocmask` (3), and
a tail of eleven functions called **once each** - three VideoOut, three libc, four service
initialisers and one unnamed `PS5Util` hash.
