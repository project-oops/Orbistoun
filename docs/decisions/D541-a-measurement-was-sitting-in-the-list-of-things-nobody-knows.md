# D541 - A measurement was sitting in the list of things nobody knows

**decided** - 2026-09-04

Fifth pass over the ask list, working the plan's three named items. Two came back clean; the
third was the opposite of what the plan predicted.

## The plan expected an overclaim and found an underclaim

`scePthreadMutexUnlock` was flagged because its question says the correspondence is inferred
"from the name **and the guest's usage**" while its `known_by` said `assumed` - the other
thirteen entries under that premise are `guest-observed`. The plan's guess was that the sentence
overclaimed.

It did not. The entry carried this, in `assumptions`:

```text
CONFIRMED ON HARDWARE: unlocking a mutex nobody holds returns 0x80020001. This was
previously a hypothesis resting on one value from an emulator that could itself have been
inferring it; a target console has now returned the same code.
```

`questions` ranks assumptions, and obSCEne's backlog 022 is generated from that ranking. **So
the queue was asking a console to establish something a console had already established, in a
sentence that said so in capital letters** - behind 4,999 calls.

And beside it, one D398 retired months ago:

> Vendor error codes **appear to be** `0x8002_0000 | errno`. Same provenance as above, and the
> same caveat: **it is a structure worth testing for, not an established encoding.**

D398 provoked seven failures across five unrelated call families on a target console and every
one came back in that shape - and *this entry's own code was one of them*. The sentence outlived
its gap, which is the failure D510 named and check 13 of the loop file exists to catch.

Both moved to `edge_cases`, keeping their reasoning; `known_by` is now `guest-observed`, which
is what its sibling `scePthreadMutexLock` has carried all along on the same basis. **That last
change is an inference, not new evidence**: the guest proceeds through 4,999 calls, which is the
one bit `guest-observed` means. It is deliberately not `measured` - the entry still lists two
open questions, and check 10 forbids one measured fact promoting an entry past them.

## The guard, and the direction it adds

`an_open_question_does_not_announce_a_measurement`: no `assumptions` line may say a console
measured something. The hardware absorption already files results in `edge_cases`; this catches
the hand-written ones. Made to fail by putting the sentence back.

What it cannot do is stated in the test: a measurement written *without* announcing itself -
"the console answers zero here" - passes, and nothing catches a stale assumption, which is
exactly what the second sentence above was. That one needed a person to notice a date.

## The other two items came back clean, with the filters stated

**The namesake test, widened past D540's single phrase.** Ten premises claim a POSIX
correspondence; the guard covered one of them. Applying it to all of them, with a candidate set
widened to several prefix strips - `sceKernelWrite` gives both `write` and `kernel_write`,
because which prefix is the vendor's cannot be decided from the name:

```text
same shape (14 fns, 872,904 calls)   0 without a namesake
behaviour follows (3)                0
resembles (1)                        0
same name (32)                       0
there is no POSIX function (9)       0 that DO have one   <- the negative claim, checked too
```

Nothing else was wrong. The guard now covers all five sentences and both directions, and asserts
that the widening did not neuter it - the three names D540 caught must still fail, or the
generosity has gone too far.

**The cross-references** - *"As `_open`."* and *"Same delivery caveat as posix_sigemptyset."* -
are inlined. Readable in a file, useless in a queue handed to somebody with a console. Six
entries now say what they mean, and two premises became members of the premises they pointed at:
three underscored spellings and five signal-set calls.

## The exception that makes the split a judgement, not a rule

Inlining the signal sentence put *"Nothing here delivers signals, so a guest that builds a set
and installs a handler will find the handler never runs"* in front of the D537 test, and it is
plainly a statement about orbistoun. I was about to move it. The implementation's doc comment
says why not:

> Recorded as an assumption rather than implied by this reporting success.

`edge_cases` are not in the ask list. An entry whose success return is a lie needs that visible
where the entry's *unknowns* are read, or the queue says nothing and the return says everything
is fine. **So the D537 split is a test, not a rule**: what orbistoun does belongs in
`edge_cases` unless leaving it there would let a success be read as working.

Reading the code before editing is what caught this. The record was right and my test was
incomplete - which is the second time this week a rule from an earlier tick needed a stated
exception rather than wider application.
