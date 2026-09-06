# D480 - The sign-extension divergence is withdrawn: it was the probe's cast, not the console's

**measured** - 2026-09-02 (user-directed plan, correcting worklogs 318-320)

Nine hardware measurements read `0xffffffff8002_xxxx`. Orbistoun builds every vendor code with
`u64::from(GuestError::vendor(..).as_raw())`, which zero-extends to `0x000000008002_xxxx`. I
recorded that as a divergence in nine outstanding entries, called it "R6's first red test", and
cited D398 - which had deliberately left the return width unencoded because it "belongs with
whichever shim returns it".

**The divergence is not there.** The probe's own source settles it:

```c
int second = scePthreadMutexTrylock(&mutex);
obs_report_measure("015-sync/mutex-recursion", "scePthreadMutexTrylock",
                   obs_type_quantity[type], (uint64_t)(int64_t)second, "code");
```

`second` is a C `int`. The leading `ffffffff` is **obSCEne widening it through `int64_t`**, and
nothing else. The check next door does the opposite -
`obs_fail_code(..., (uint64_t)(uint32_t)held)` - which is why the same function appeared both
ways in the same capture and why I called one of them authoritative. Neither is: they are two
casts of one 32-bit value.

## What could not have been measured, and why

A C prototype returning `int` reads `eax`. **The other thirty-two bits of `rax` were never
observed**, by either check, because nothing in the probe was in a position to look at them.
The question D398 left open is still open, and these records were never capable of closing it.

Settling it needs a probe that captures the whole register - an assembly thunk that calls the
function and reports `rax` verbatim, rather than a C declaration that truncates before the
value reaches any recording. That is a real piece of work and it is the only thing that would
answer the question.

## What the records do say, which is better than what I claimed

Read at the width they were taken, **every one of the nine is a value orbistoun already
produces**: `0x80020001` is `errno::NOT_OWNER`, `0x80020010` is `BUSY`, `0x80020016` is
`INVALID`, `0x80020002` is `NO_ENTRY`, `0x80020003` is `NO_SUCH`.

So the nine were never divergences. One of them is now an assertion that passes - releasing a
lock nobody holds answers the measured code, compared at thirty-two bits because that is the
width the measurement exists at. The other eight stay outstanding for a reason that is
actually true: reproducing the *condition* needs the check's setup, not a different return
value.

## The mistake, named

**I read a value and inferred a mechanism.** `0xffffffff80020001` looks exactly like a
sign-extended errno, that reading fit D398's open question, and it made an appealing story - a
subtle ABI bug nobody had noticed. What it needed was one look at the line that produced it.

The project already has this rule twice over: "an intervention that moves a wall is not a
diagnosis" (D227), and "a message naming a cause must come from the branch that determined it".
This is the same failure in a report: **a claim naming a cause must come from the thing that
measured it.** A hex value is not a mechanism, and the source that emitted it is one grep away.

The nine entries also went into a work queue as facts, which is worse than putting them in
prose - a queue is read as settled work. That is the specific harm and the reason this is a
decision rather than a footnote.

## Consequences

- The nine outstanding entries are rewritten to say what is true.
- `015-sync/mutex-unlock-unheld` is claimed and asserted.
- Worklogs 318, 319 and 320 carry a correction banner rather than being rewritten, which is the
  convention D469 set.
- **The width question returns to being open**, credited to D398 where it started, and its
  answer needs a register-level capture nobody has built.
