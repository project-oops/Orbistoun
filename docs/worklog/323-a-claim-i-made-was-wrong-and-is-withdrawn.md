# 2026-09-02 - (/loop) The sign-extension divergence was mine, not the console's

```
hardware claims asserted    6  ->   7
outstanding                21  ->  20
tests                    1950  -> 1951
```

Went to settle the return-width question - nine outstanding entries, and the plan said it
wanted a decision before a sweep: per function, or the whole vendor family? **The answer is
neither, because the divergence is not there.**

## What the records actually say

Nine measurements read `0xffffffff8002_xxxx`. Orbistoun answers `0x000000008002_xxxx`. I
recorded that as the console sign-extending where orbistoun does not, cited D398 - which had
deliberately left the width unencoded as belonging "with whichever shim returns it" - and wrote
it into a work queue as nine items.

The probe's own source settles it in one line:

```c
int second = scePthreadMutexTrylock(&mutex);
obs_report_measure(..., (uint64_t)(int64_t)second, "code");
```

`second` is a C `int`. The leading `ffffffff` is **obSCEne widening it**, and nothing else. The
check next door does `(uint64_t)(uint32_t)held` instead, which is exactly why the same function
appeared both ways in one capture - and why I declared one of them "authoritative" rather than
noticing that neither could be.

**A prototype returning `int` reads `eax`.** The other thirty-two bits were never observed by
either check, so those records were never capable of answering the question. D398's question is
still open, and settling it needs an assembly thunk that reports `rax` verbatim - something
nobody has built.

## The mistake, named, because it is a pattern

**I read a value and inferred a mechanism.** `0xffffffff80020001` looks precisely like a
sign-extended errno; the reading fitted an open question; and it made a good story - a subtle
ABI bug nobody had spotted. What it needed was one grep at the line that produced it.

This project already carries the rule twice: "an intervention that moves a wall is not a
diagnosis" (D227), and "a message naming a cause must come from the branch that determined it".
Here it is in a report: **a claim naming a cause has to come from the thing that measured it.**

The aggravating part is where it went. Nine entries into a *work queue*, which is read as
settled work - worse than prose, because prose invites doubt and a queue invites action. That
is why this is D480 and not a footnote.

## What the records say instead, which is better

Read at the width they were taken, **every one of the nine is a value orbistoun already
produces**:

| measured | is |
|---|---|
| `0x80020001` | `errno::NOT_OWNER` |
| `0x80020010` | `errno::BUSY` |
| `0x80020016` | `errno::INVALID` |
| `0x80020002` | `errno::NO_ENTRY` |
| `0x80020003` | `errno::NO_SUCH` |

So they were never divergences. One is now an assertion that passes: releasing a lock nobody
holds answers the measured code, **compared at thirty-two bits**, because comparing sixty-four
would be comparing against obSCEne's cast rather than against the console.

The other eight stay outstanding for reasons that are true this time - each needs its check's
*condition* reproduced (a mutex type mapping, a direct-memory allocation, a loaded module), not
a different return value.

## State

`cargo test --workspace` green - **117 suites, 1951 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. `also hardware` reports 38 measurements, 27 constant and
accounted for: 7 claimed, 20 outstanding.

Worklogs 318, 319 and 320 carry a correction banner rather than being rewritten, which is the
convention D469 set - the wrong reasoning is worth more visible than deleted.

Nothing committed. The day holds worklogs 292-323 and D466-D480.

**Next**: `strtok`, which needs a sequence case shape because its answer depends on state
carried between calls. Then the mutex type mapping, which would move three more measurements
from outstanding to asserted and is the cheapest of the eight.
