# The shell button had nowhere to go


A shell was scoped as four things - browse the library, boot into it, press the system
button in-game, quit back to it. Reading the protocol before writing any of it split the
list in half, and not along the line the scoping conversation had drawn.

Two of those are a front-end: host-side drawing, no guest involvement, no dependency on
graphics or audio or pad emulation at all. Two are an ABI contract, and they were blocked
on something that is not a subsystem - it is the *absence* of one.

Guest code runs in a child process (D032). `Request` was send-once-then-listen: read a
request, run the guest to completion, stream events back. The only control while a run was
in flight was `Stopper`, which is `TerminateProcess`. So a shell action arriving mid-run sat
unread in the pipe until the run it was meant to interrupt had finished. Nothing was broken.
There was no path.

That is why the prerequisite list in the scoping conversation was wrong. It named controller,
GPU and sound. The actual prerequisite was a channel, and it was invisible precisely because
missing plumbing does not appear on a list of subsystems.

### What the provenance rule did to the design

The interesting half was not the channel. A title learns it was interrupted by draining an
event queue, and there is no lawful source here for what those events are *numbered*.

Three options, two of which are the same mistake. Invent a plausible code - principle 3's
forbidden case exactly. Pick zero - the same act, humbler clothing. Or separate the meaning
from the number, which is what happened: `SystemEvent` is our vocabulary and carries no
codes, `Delivery` maps meaning to code and ships empty, and an event with no measured code is
withheld *and counted*. A run says `4 withheld for want of a measured code (backgrounded x2,
focus-lost x2)` instead of a shell that appears to work.

One detail that took a second pass: the undeliverable case is decided at `post`, not at the
far end. Queueing an unmapped event would park it at the head and deny the guest every
deliverable event behind it - the feature failing worst exactly where it was working best.

`sceSystemServiceReceiveEvent` is therefore still not declared, though its name is
hash-confirmed in our own database. Two things are unmeasured: the value meaning *no event
pending*, and the layout of the structure an event is written into. Both are probe work on
real hardware.

### The reframe

This reads as a limitation and is closer to the opposite. `orbistoun-systemservice` has always
answered "what language, which button confirms" with a documented placeholder because *"nothing
here knows what any of these parameters mean; they are console settings."*

They are console settings, and a shell is where a person sets them. `Settings` is the first
thing in the tree with standing to answer, because a console setting is a fact about what the
owner wants and the owner is right there. What was missing was never knowledge. It was
somebody entitled to decide.

### Three things that bit

`StdinLock` is not `Send` - it holds a `MutexGuard`, so a lock taken in worker mode could
never reach the reader thread. Worker mode wraps the unlocked handle instead.

A `perl` substitution against the protocol enum silently matched nothing and reported
success. Same failure as the one in the repair work: an edit with no assertion is not an
edit. Switched to a tool that errors on no-match, which caught it immediately.

`D306` was already taken. The other session had written up to `D309` while this was in
flight, so a decision number chosen from memory pointed at somebody else's entry - a
citation that would have been wrong in a way nothing checks.

### Two guards that reported more than they measured

**The progress verdict invented a position it did not have.** `fault: None` means a run did
not fault. `compare` scored that as an ordering anyway - a run that *stopped* faulting
reported `BACK`, *"reaching less of the interface than it did"*, and one that *started*
faulting on the same imports reported `FURTHER`, *"executed code it could not reach before"*.
Both sentences are false and `FURTHER` is the word this project steers by. Thirty lines
above, `describe_end` already refuses to pretend a missing fault is an address; the two
halves of one file disagreed about the same absent value, and **neither arm had a test**.
Fixed, with D301's other half: a run that ends without a fault now says the position measured
nothing (D309).

**And `compat record` waved a propped-up run through.** Its guard reads `default_return`;
`learned.toml` leaves that at `unimplemented` by design and puts its answers in per-function
overrides. So the feature built this session to carry findings home drove straight through
the guard written to stop exactly that shape of result.

The repair was not a stricter guard. A refusal is a place the loop stops and waits for a
person, and the refusal only existed because one best-ever entry could not hold two kinds of
result. The record now holds both - `[status]` for the emulator as it stands, `[experiment]`
for the furthest it got while being helped - each compared only against its own slot, and
nothing is refused on policy grounds any more (D312).

### The surprise

Both bugs were **unasserted branches**, and both were found by asking a question about
something else - one about why a test was slow, one about where the loop's gains go. The
tests written for them fail loudly against the old code, which is the only reason either is
now a fact rather than a belief:

```
a_run_that_stopped_faulting...  : reaching less of the interface than it did
a_run_helped_by_named_overrides : and it was still helped along
```

### Not fixed, and worth knowing

Nothing carries a run's findings into the repository on its own. `compat record` is still a
command a person types, and `compat/*.toml` has been untouched since 2026-08-23 - so
everything established about PPSA02664 since then is absent from the repository's own record
of it. Removing the policy refusals is what makes automatic recording *safe*; wiring it into
a run is a behaviour change nobody has asked for yet.

### A machine can now hand over what it found

`orbistoun-submit` is what a submission is: measurements, title results, a manifest naming
the build. Nothing else. Traces and run reports are excluded deliberately - they are inputs
rather than claims, they are large, and they carry far more of a title than a result needs
to.

The crate depends on the measurement format and the title record and nothing else. No
loader, no emulator, no model runtime, enforced by cargo rather than by care: a bundle
carries claims and cannot smuggle behaviour.

It could not have been built yesterday. A mining run is a run under a measured policy, and
the recording refused those outright - so a distributed contributor had to pass `--force`,
and their entry then contaminated the honest number. Routing results into two slots is what
made theirs carryable at all.

Exercised end to end against this machine: one measurement, six title results, checked back
against itself with silent agreement, then against a bundle claiming a title never run here -
reported as unmeasured rather than as a contradiction, which is the distinction the whole
`known_by` ladder exists to hold.

### The bug it shipped with, and how it was found

`submit check` printed *"6 title result(s)"* from a bundle carrying seven. It read the number
off the manifest.

A manifest is a claim by whoever sent it, so quoting it back reports the sender's arithmetic
as the receiver's measurement - this log's oldest failure, arriving in the newest code, in
the one command whose entire job is to not trust what it was handed.

Found by running it against a bundle edited by hand, not by writing it carefully. That is the
whole of it: every guard in that file is now one somebody has watched refuse something.

