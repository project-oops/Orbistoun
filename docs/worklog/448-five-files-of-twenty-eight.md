# 448. Five files of twenty-eight

**2026-09-08** - directed, continuing 447

## What was done

447 taught `orbistoun-probe` to read `measure` records. Following that thread found a second
reader that had been reading them all along - `orbistoun-gen measurements`, which writes the
table `tests/hardware.rs` asserts against - with its own hand-rolled field split and a much
worse problem than duplication.

**It was reading five capture files out of twenty-eight.** One directory of the sibling
project's three, and one extension of its two. A session transcript is `.txt` and a report is
`.obs.log`; pointed at a directory of reports the walker found nothing at all. Two of the
captures the committed table names as its own sources are not in the directory it reads - they
moved to an archive and nothing noticed, because a capture that has gone missing contributes no
measurement and therefore no complaint (D609).

| | before | after |
|---|--:|--:|
| captures read | 5 | **28** |
| distinct measurements | 227 | **374** |
| constant across every run | 227 | **262** |

## The corpus is not one machine

Nineteen measurements stopped being platform constants, and the reason for seven of them is
worth stating plainly: `120-measure/cpuid:cpuid:signature_eax` is `0x740f12` in some runs and
`0x840f60` in others, with `model`, `stepping` and two feature words following it.

**Two console generations.** This morning's reports say so out loud with their new `title/ps4-bc`
context, but the older captures were already mixed and the table asserted CPUID fields as
platform constants regardless.

Three more are `130-layout/direct-memory-query-flags`: `flags-0` answered `0x0` sixteen times and
an error three, `flags-2` and `flags-4` answered the invalid-argument code eighteen times and
`0x0` once. Whether a query with nothing allocated finds something is a property of the machine's
state, not of the flag.

And `mxcsr:raw` is `0x9fe0` twelve times, `0x9fc0` three. The difference is bit 5, the sticky
precision flag - so it is not a property of the platform at all, it is whether the console's own
startup did inexact arithmetic before the probe looked. **Orbistoun installs `0x9fc0`, which is
now exactly what three console runs show.** D486 chose that on reasoning; the wider corpus
measures it.

Both assertions resting on a demoted value now claim membership of *every* value any run
reported, which is what `Measurement::values` was built for and had no caller until today. The
mxcsr one also asserts that every bit the runs differ in is a status bit, so a configuration
difference cannot hide inside the permitted variation.

## The gate fired at four times its usual scale

`every_constant_measurement_is_claimed_or_declared_outstanding` exists so a hardware run becomes
work rather than a file nobody reads. It failed with **110 unaccounted constants** - the
mechanism working exactly as designed.

Triaged:

- **54 opaque**: console addresses (orbistoun's own bases are all in `docs/ADDRESS_MAP.md`, and
  none of them is these), one machine's user id and interface flags, handles from the console's
  own numbering, and the kernel's payload handoff, which is below the user-space boundary this
  project works inside.
- **56 outstanding**, and the split inside it is the finding. 25 are conditions of functions
  orbistoun implements that nothing asserts yet - ordinary work. The other **31 are symbols the
  console resolves that this project does not declare at all**: the whole of `sceMouse*`,
  `sceKeyboard*`, `sceAudiodec*`, `sceAjm*`, and six of seven extended `scePad*` entries.

That last group is the reason the exercise was worth doing. They were not hard problems being
deferred; they were gaps nobody knew about, and they are now named, counted, and in a file the
gate reads. Principle 6 says the order they get done in, and it is not this one.

## Surprises

- **The second reader was the older one.** `orbistoun-gen` had been parsing `measure` records
  since before `orbistoun-probe` had a variant for them, so 447's "the largest record kind is
  read by nothing" was true of one reader and not of the project. Both statements were checked;
  neither reader knew about the other.
- A capture directory holds a raw kernel log that is not valid UTF-8, and reading it aborted the
  whole run - so one unreadable file cost the other twenty-seven captures. Skipped by name now,
  never silently.
- Removing 19 declarations with a regex left 16 orphaned reason strings behind, and the compiler
  caught every one. A two-element tuple is a good shape for exactly this.

## And the first eight turned into four wrong answers

`016-syncbounds` entered the table for the first time, and orbistoun disagreed with half of it.

**`sceKernelPollSema` ignored the count it was asked for** - it took one, whatever the second
argument said. Asking for two where one is left answers busy on the console; orbistoun answered
ok *having taken the one*, so a caller believing it holds two releases two and the count runs
away upward from there.

**`sceKernelPollEventFlag` read one bit of the mode.** `0x00` names neither `and` nor `or` and
the platform refuses it; orbistoun read it as `or`. That was right in three cases out of four
**because the wrong branch usually produces the right answer** - a pattern failing an `and`
normally satisfies an `or` - which is precisely the bug nothing but a measurement finds (D610).

Fixing the mode then broke `measured_refusals`, correctly: the console answers the bad-handle
code to a poll on a handle it never issued *even when the mode is also invalid*. Two
measurements together fix something neither states alone - the handle is checked before the
mode - and `sync::event_flag_exists` exists so that order is written down rather than emerging
from how the code happens to be arranged.

All eight now assert, in two new tests, and moved from outstanding to claimed.

## The parser required a prefix the probe does not always write

Writing those assertions, one panicked on `.expect("a code is a number")`. `Measurement::value`
required a `0x`, and two of the probe's checks do not write one - `000-hw/sw-version` records
`0`, `000-hw/tsc-frequency` records `1596300187` - so every measurement from them answered
`None`.

**The panic is the good outcome.** A caller writing `.expect` finds this in one run; a caller
writing `.unwrap_or(0)` never finds it and quietly asserts against a zero nobody measured. It
parses both bases now, refuses a word, and is called `parse_number` - a function called
`parse_hex` that also reads decimal is the next person's bug (D611).

Reading decimal made a third frequency visible for one quantity: 1,596,300,187 against
1,596,300,179 and 1,596,300,174. Thirteen hertz in 1.6 GHz, which is the per-boot calibration
`the_counter_frequency_matches_the_console` already describes.

Six more measurements now assert. `getifaddrs` and `sceUserServiceGetInitialUser` agreed first
time; `sceKernelSyncOnAddressWake` answering `0` with nothing waiting is the one worth pinning,
because the tempting implementation refuses it - a wake that found no waiter did nothing, and a
call that did nothing looks like a call that failed.

**`sceKernelGetSystemSwVersion` disagreed and is right to.** A console answers `0` because it has
a software version; orbistoun refuses when none is configured, which is what D420 chose over
answering a made-up one. Claiming it needs a machine presented first, and `machine::present` is a
process-wide `OnceLock` - a test setting it would decide what every other test in that binary
sees, by running order. It goes back on the outstanding list carrying that, which is more than it
carried before somebody tried.

## One measurement that must not be claimed

`031-stackattr/fresh-attr-names-no-stack` claims cleanly - a fresh attribute names no stack
address and a default size of `0x10000`, which orbistoun already answers because D585 read the
same measurement when the code was written. The test found its own bug getting there: every call
in that family takes a `ScePthreadAttr *`, the address of the caller's variable, and passing the
handle itself has the implementation read a word from wherever that handle points. An access
violation rather than a wrong answer, which is the good kind.

**`031-stackattr/address-is-the-base:sceKernelIsStack:is-stack` is a different matter.** The
console answered `0` for an address inside its own stack; orbistoun answers `1`. Changing
orbistoun to agree would be matching a number rather than a behaviour, and `libkernel.toml`
already records that this check reported a stack address and a static one alike - so what the
`0` says about `sceKernelIsStack` is unsettled. It stays outstanding carrying that, because the
work is reading the probe's check, not writing an assertion.

That is the case the whole claimed/outstanding split exists for: a measurement is evidence, and
a measurement nobody has understood is not yet a specification.

## And reading that check took ten minutes

`sceKernelIsStack` is not a predicate. The probe calls it
`sceKernelIsStack((void *)&frame, &low, &high)` - **three arguments**, a status return, and the
bounds in the two words after the address. Orbistoun declared arity 1, answered `1` for an
address in the stack and `0` for one outside, and wrote no bounds at all.

| | orbistoun | the console |
|---|---|---|
| arguments | 1 | 3 |
| a local | `1` | `0` |
| a static | `0` | `0` |
| bounds written | none | `low`, `high` |

`010-kernel/is-stack` fails in **twenty-three runs of twenty-three**, always with *a stack
address and a static one were reported alike*, value `0x0`. That check was written for a
predicate, so it reads a successful call as a broken function - and the knowledge entry recorded
it as a fact about the console's defect rather than about ours. The bounds `031-stackattr`
records are two mebibytes apart, exactly the stack size the same run read off the thread's own
attribute (D612).

So a guest asking where its stack is got an inverted flag and two words of its own uninitialised
memory back.

**Two harnesses caught the fix landing.** `tests/posix.rs` fills unspecified argument registers
with `0xDEAD_BEEF_DEAD_BEEF` rather than zero, precisely so a function reading an argument it was
not given is caught - and it faulted immediately on the new `write_word`. Then
`declared_arity_and_recorded_arity_never_disagree` refused the change until the knowledge entry
moved with it. A helper padding with zeroes would have let both halves land silently.

## The wall moved, and it was nearly credited to the wrong thing

Running the wall title afterwards:

```text
imports  196 distinct (+3), 478848 calls (+11395)
fault image+0x39f7c   (was image+0x1389269)
verdict  MIXED    more of the interface reached, but along a different path
```

Three more distinct imports and a different fault site. The tempting sentence writes itself, and
it would have been wrong twice over.

**First check: the limit.** The committed record was taken at 12 seconds and this run had 20, so
the two do not compare. Re-run at 12: still 196, still `image+0x39f7c`. The limit is not it.

**Second check: is the guest calling anything I changed?** I wrote here, first, that it was not -
that the call list contained no `sceKernelIsStack`, no `PollEventFlag`, no `SyncOnAddressWake`.
**That was wrong, and the way it was wrong is worth more than the answer.** The command was
`orbistoun-cli run … --top 400`; `run` has no `--top`, so what I grepped was a four-line clap
usage error redirected into a file. Every pattern missed, and an empty search reads exactly like
a search that found nothing.

The same shape as the `2>&1 > file` mistake earlier this session: a command that did not run,
read as evidence about the thing it did not measure. The fix is the same too - check the size of
what you are grepping before you believe it.

`orbistoun-cli worklist --top 400` is the command, and the guest calls **all** of them:

| call | calls | sites |
|---|--:|--:|
| `sceKernelCreateEventFlag` | 591 | 7 |
| `sceKernelWaitEventFlag` | 70 | 3 |
| `sceKernelSyncOnAddressWait` | 26 | 1 |
| `sceKernelIsStack` | 14 | 2 |
| `sceKernelSyncOnAddressWake` | 13 | 1 |
| `sceKernelPollEventFlag` | 9 | 1 |

So this iteration's changes **are** on the path. Which one was settled by taking each out again:

| | distinct imports | fault |
|---|--:|---|
| `sceKernelIsStack` as a predicate | 193, 193 | `image+0x1389269` |
| corrected to three arguments | 196, 197, 197 | `image+0x39f7c` |

Reverting that one function - everything else this session changed left in place - puts the
guest back on the old number and the old fault site exactly, and restoring it brings the new ones
back. The event-flag mode refusal was tested the same way and moves nothing, despite 70 calls to
`sceKernelWaitEventFlag` and 9 to `sceKernelPollEventFlag` on the path.

**Three more of the interface reached, every run, and the wall in a different place.** Not
"further" in the ordinary sense - `0x39f7c` is a *lower* offset, so the guest dies earlier in the
image while reaching more of it, and the report says the positions do not compare. What compares
is the interface.

## Both titles now fault at the same offset

`image+0x39f7c`, in PPSA02664 and PPSA03416 alike - two different titles, the same image
offset, and both Unity. That is either one statically linked routine appearing at the same place
in two similarly built executables, or one failure mode reached two ways. Either way it is worth
more than a title-at-a-time investigation: a wall shared by two titles is one wall.

PPSA02664 itself has an earlier record of 215 imports at `image+0x42c76`, so it reached further
once and no longer does. That is a separate thread and a real one.

## What is at `image+0x39f7c`

The instruction, read out of the running guest with `ORBISTOUN_WATCH`:

```text
31 c0        xor  eax, eax
85 d2        test edx, edx
74 15        je   +0x15
48 8b 76 30  mov  rsi, [rsi+0x30]
ff ca        dec  edx
8b 14 96     mov  edx, [rsi + rdx*4]   <- faults here
```

A structure walk. The guest loads a pointer from **offset `0x30`** of an object and immediately
indexes through it - and at the fault `rsi` holds `0x542e2e00776f6c66`, which is not a pointer.
Little-endian, its bytes are `66 6c 6f 77 00 2e 2e 54`: **`flow\0..T`**, the tail of a string in
a string table.

So the field at `+0x30` that should hold a pointer holds text. The walk landed in string data,
which is what a structure being read at the wrong offset - or a structure nobody filled - looks
like from inside.

`rbx` holds `0x7fff0001` at the fault, which is orbistoun's `Unimplemented` placeholder sitting
in a callee-saved register. Something the guest called was not implemented and its answer is
still live.

**One thing that looked like a finding and is not, yet.** `r14` points at
`0x74000ae40100`, and watching that address shows a region that did not exist when the guest
started and is all zeroes at the end. That is suggestive and it is not evidence: mapping
addresses still vary between runs, so the address from the faulting run and the address watched
in a later run are not known to be the same object. Establishing that needs the mapping sequence
to settle first, which is already on this list.

## The call just before it, and two things that answered without knowing

Dumping the arguments of the call the report names as immediately preceding the fault:

```text
sceKernelWaitEqueue
  arg0 = 0x5e2d0000ee40    the queue
  arg1 = 0x610084800f80    the event array - all zeroes
  arg2 = 0x1               one event wanted
  arg4 = 0x0               no timeout: wait indefinitely
                        -> 0x0    success
```

**The guest asked to wait indefinitely for one event and was told immediately that it had
succeeded**, with the array it passed never written. 3,853 waits against 44 flips in one run.
That is `kevent(2)`, which blocks until an event is ready or the timeout elapses, and it is the
D171 shape exactly: an out-parameter left as the caller set it, under a success code.

The queue has a condition variable now, `post_event` signals it, and nothing arriving in time
answers `ETIMEDOUT` rather than success. **It did not move the wall** - 197 imports either way,
same fault - and it fixed a wrong answer, which is its own reason (D613).

**And then the trace started lying.** With the wait actually waiting, the same evidence line read
`sceKernelWaitEqueue(...) -> 0x74000086ec30`, which that call cannot return - it answers `OK`, a
vendor errno, or the invalid-argument placeholder, never a mapping address. The value changed
between runs while staying inside `MAPPING_BASE`, which is what a *memory-mapping* call answers.

The call record is a ring. D571 fixed the writing side - a call returning after its slot has been
recycled must not store its answer there - but nothing cleared the flag when a slot was **taken**,
so a call still running in a recycled slot reported the answer of whichever call held it last.
Invisible while every call returned promptly, because the window was a few instructions wide. A
call that blocks holds the slot open, and the defect walked straight into the one record a person
reads at a wall.

One store, ordered before the sequence is published. The line now reads
`sceKernelWaitEqueue(0x5e2d0000ee40) from 0x400000f53413` - no arrow, because there is no answer
yet, which is what `recorded_return` has promised since D459.

Both defects are the same sentence: a value that was *available* reported as a value that was
*established*. Neither was a wrong calculation; both were a missing "I do not know yet", and in
both cases the honest state was already representable and simply not written.

## The loop named the next function itself

With the wait honest, the run's own ranked findings answer the question the wall poses:

```text
! libSceAgcDriver::sceAgcDriverAddEqEvent was called 2 times and nothing implements it
    arg0 = 0x5e2d0000ee40
    arg0 = 0x5e2d0000efe0
```

**`0x5e2d0000ee40` is the queue the render thread blocks on.** The guest registers a driver
completion against it through a function nothing implements, so nothing is ever registered and
nothing can ever be posted. `sceVideoOutAddFlipEvent` - which orbistoun does implement - registers
against a *different* queue, which is why flips post and this wait still never completes.

That is the loop working as designed: the fault names the calls before it, the calls name a
handle, and the handle names the one unimplemented function whose argument matches.

**Implementing the registration would be wrong on its own**, and the finding's own arrow says
why: a queue registered and never posted to is the same wait, arrived at more slowly. What has to
be settled first is *what completes driver work in an emulator with no GPU execution* - the
question D560 answered for flips, where a flip completes the instant it is accepted because there
is no scanout, and which nobody has answered for this. That is a concept rather than an
implementation, so it is written down and not guessed at.

Recorded with `orbistoun-cli learn` as `assumed`, carrying the two argument-zero values, the
shape it mirrors, and the assumption that argument zero is the queue rather than a driver handle -
which is evidence from two calls, not a specification.

The second finding is corroborating: `sceAgcDcbResetQueue`, also unimplemented, is passed
`arg5 = 0x400001720a52`, which points into the string table holding
`duplicate\0invalid signature\0seq…`. That is the same table `0x542e2e00776f6c66` - `flow\0..T` -
is a fragment of. The structure walk at the wall is reading a table the guest is passing around
as a pointer argument to calls that do nothing with it.

## The flake was the finding

Three timing tests have failed intermittently under the gate all session and passed on their own
every time. The obvious reading is a test that is too tight. The message says otherwise:

```text
and it did not give up early: 79.8797ms
```

Eighty milliseconds asked for, 79.88 waited. **The test is right and the primitive is wrong.**
`WaitTimeoutResult::timed_out` is the platform's verdict on the platform's own timer, which here
is coarser than `Instant` and can report a timeout a fraction of a millisecond early - more often
under load, which is why it clustered in the gate. So `sceKernelWaitSema` with a ten-millisecond
timeout could answer `ETIMEDOUT` after 9.9 (D614).

The fix is one condition: ask the clock as well as the flag, and let an early wake go round the
loop again with a freshly computed remaining. Five consecutive sync runs and a full workspace run,
no failure.

What it was nearly is a loosened assertion - green gate, guest-visible defect buried under a
tolerance, and a test left saying the deadline is approximate when it is not.

## The report says it in one line now

The event-queue line has listed registrations since D524, which cannot tell a queue nobody uses
from a thread stuck on one - both read as a name and a count. It carries waits and deliveries
now, and calls the starvation out rather than leaving it to arithmetic:

```text
"eq to wait flip"   (1 registered, 0 waits, 0 delivered)
"flip equeu"        (2 registered, 0 waits, 0 delivered)
"UnityFTMFlipQueue" (0 registered, 1 waits, 0 delivered)  <- waited on, never delivered
"EOP QUEUE"         (0 registered, 0 waits, 0 delivered)
```

**The guest waits on the one queue with no registrations, and never waits on either queue that
has them.** That sharpens the afternoon's reading: it is not that AGC completions are missing in
general, it is that the queue the render thread blocks on has no producer at all, while the two
queues orbistoun does feed are ones this guest never asks about (D615).

It also confirms the fix has to be more than a registration. `post_event` matches by identifier,
`sceVideoOutSubmitFlip` posts under the port handle, and the AGC call carries none - its second
and third arguments are both zero. A registration no post can match is the same wait, reached
more slowly.

**PPSA02664 and PPSA03416 produce the identical four queues, the identical traffic and the
identical fault.** Two titles, one Unity engine, one wall - so a measurement against either is a
measurement about both.

## Three of its six arguments are not arguments

The handle in the report line joins it to the argument dumps, which is what it is for - and the
join says the two `sceAgcDriverAddEqEvent` calls target exactly `UnityFTMFlipQueue`
(`0x5e2d0000ee40`, the one the render thread blocks on) and `EOP QUEUE` (`0x5e2d0000efe0`). The
two queues orbistoun *does* feed are never waited on.

Their fifth argument read `0x17` and `0x20` - two calls registering two kinds of queue, with what
looks exactly like an event type. **I was one sentence from writing that down.** Two runs of one
build say otherwise: `arg5` reads `0x80` and `0x60` next time, and arguments three and four are
*host* addresses that change every run. Only argument zero is stable, with the two zeroes beside
it.

So the call takes at most three arguments, the declaration's six is orbistoun's own guess, and
the surplus registers hold whatever the caller last put there. The dump prints six because six is
what it captures.

**The tell was there in one run and did not read as one.** `0x7ff7e75fcab0` is a host address;
every family orbistoun hands a guest is in `docs/ADDRESS_MAP.md` and none looks like that. The
report says "in no span this run published as readable" - true, and it reads as *the guest passed
a pointer we cannot resolve* rather than *this register was never set*. Worth distinguishing, and
not distinguished yet.

The knowledge entry carries the measurement now, so the next person to look at `arg5` finds out
before building on it.

## The tell is a sentence now

`describe_unreadable` compares an address-shaped value against the **envelope** of everything a
run published - lowest region base to highest end - and says when it falls outside:

```text
arg3 = 0x7ff68140cab0 -> in no span this run published as readable, and address-shaped
                       - and outside every region this run gave the guest
                         (0x400000000000..0x74000d080000)
```

Two findings that read identically before: *a pointer into something this run never declared*, and
*a register the call never set*. The first is inside the envelope; the second is not.

It does not claim more than that. A guest can compute a wild pointer, and `orbistoun-abi` still
hands one out in two places - so it is a fact about where the value sits and the reading is left
to the reader. The test asserts **both** halves, because the useful sentence is the one that does
not always fire, and it was made to fail by forcing the condition true.

## The mapping record was asking the wrong thread

Chasing the standing "mapping sequence still varies" item, the trace read:

```text
26  call 459349  0x740001980000 +0x80000  r arena      during libc::memcpy
31  call 459433  0x740001b00000 +0x40000  r asked-for  during libkernel::scePthreadAttrInit
```

**`memcpy` maps nothing.** Those mappings were made by `sceKernelReserveVirtualRange` and
`sceKernelMapDirectMemory` on one guest thread and labelled with whatever a *different* thread
had most recently entered - because `last_call` reads the whole recording ring and answers with
the newest call by sequence, across every thread. Right for a fault handler asking what the
process was doing; wrong for an implementation asking what it is inside. While the guest was
single-threaded the two agreed, so the difference had nowhere to show (D616).

`current_call` is a thread-local word, set around the handler and restored after - a stack,
because calls nest through callbacks. `last_call` keeps its own meaning, now written down beside
it. Two questions that were being answered by one function are two functions.

```text
26  call 459728  0x740001e00000 +0x80000  r arena      during libkernel::sceKernelReserveVirtualRange
32  call 459978  0x740002080000 +0x400000  r asked-for  during libkernel::sceKernelMapDirectMemory
```

**And the item it was chasing retires with an answer rather than a fix.** Two runs agree on the
first twenty-six mappings exactly and diverge from where the second thread starts allocating; the
count differs too, 61 against 57, because each thread gets as far as a wall-clock budget allows.
That is a multithreaded guest under a time limit, not a defect - removing it needs a
deterministic scheduler, which is a concept nobody has proposed. What every progress comparison
actually rests on, the distinct import count, has been stable since D604.

## A second batch arrived, and nothing had to be built to read it

Six files an hour after the last set, replacing them, with twelve sections nobody had captured
before - sockets, mouse and keyboard reachability, video and audio decode, an unheld-mutex unlock,
`kern.osrelease`. `orbistoun-gen measurements` picked all three up from a directory it already
reads, `names --from-report` took the export table, and the coverage gate failed with a list.
That is what the morning's work was for, and this is the first time it ran unattended (D617).

| | after D609 | now |
|---|--:|--:|
| distinct measurements | 374 | **429** |
| constant across every run | 262 | **304** |

Forty-two new constants and **nothing previously claimed demoted** - the claim-check passed first
time, which it did not last time.

**Nine of the forty-two are a subsystem orbistoun has not started.** Nothing in `libSceNet` is
declared here, and the console answered for all of it:

```text
sceNetBind        bound             0x0
sceNetListen      listening         0x0
sceNetRecv        connected-return  0x80410123
sceNetRecv        listener-return   0x80410139
```

A socket with nothing to read answers **one code when connected and a different one when
listening** - a distinction a reimplementation flattens without noticing, because both read as
"would block". And `0x8041` is libSceNet's own error space, not the `0x8002` kernel one every
existing vendor code here uses. Recorded as `measured` in `libSceNet.toml`, citing the check, for
a function that does not exist yet.

That is the inversion the whole apparatus is for: the oracle arrives first and the implementation
gets written against it, rather than being written against a guess and corrected when a title
disagrees.

The export table did not move - 2,443 exports, 215 unnamed, same as an hour before. Whether the
*addresses* match is unverified: the previous batch was replaced rather than archived.

## And that aside was the real finding

A report directory being overwritten is not just a limit on comparison. `orbistoun-gen
measurements` rebuilt the table from whatever was on disk, so the previous batch's observations
were **deleted from the committed table by a successful run**. `20260908-094705` appears in it
zero times now, and every number D609 drew from it is unreproducible.

The dangerous half is which way it moves a claim. `constant` means *every run that took this
agreed*, and it decides whether anything may assert a value. A measurement marked non-constant
**because two batches disagreed** becomes constant again the moment one batch is deleted - and
the coverage gate then demands it be claimed, and a claim is what it gets. Evidence disappearing
made a conclusion safer, silently, in a run that reported success (D618).

Measured by taking the fix back out - regenerating with only the directory that still has its
captures:

| | measurements in the table |
|---|--:|
| before | 429 |
| without the fold | **38** |
| with it | 429 |

A 91% loss, no error, exit zero.

The committed table is folded in beside the captures now, which is the rule `write_symbol_db` and
`write_wanted` have both followed since D074 for exactly this reason: each run sees part of the
corpus, and writing only what it saw discards the rest. The measurement table was the one that
did not. Idempotent, and the guard was made to fail by dropping the `disagreed` parse - the
assertion that catches it is the one about the contradiction surviving, because losing a row is
visible and losing a row's contradiction is not.

**The 094705 batch cannot be recovered.** It was already gone when this was found. D609's
conclusions stand in the log; the evidence behind the numbers it quotes does not.

## The same disappearance, one file along

The export table has the identical problem and D618's fix does not cover it, because D609
deliberately kept it out of the measurement table - 2,443 rows whose subject is a hash and whose
value is where the kernel happened to put it would make a table of checkable claims four fifths
symbol table.

So the 215 hashes a console exports and this project cannot name existed **only while a batch of
captures was on disk**. About an hour. They are the one input no amount of local work reproduces:
a hash from an import table is one a title asked for, and one from an export table is what the
platform offers whether or not anything ever imported it (D245).

`symbols/exported-unnamed.txt` now, beside `wanted.txt` and never inside it - two lists counting
different things, with the file name saying which, both going through `wanted_now` so "still
unnamed" cannot come to mean two things. A run that sees no reports writes nothing and destroys
nothing, which is what every ordinary `./bin/orbistoun names` does and the case that matters most
(D619).

## A question put to the guest, and the answer was no

Before implementing anything for `UnityFTMFlipQueue`, there is a narrower question a flip can
answer on its own: **if that wait completed, what would the guest do next?**

`ORBISTOUN_FLIP_TO_ALL` is a diagnostic that makes `sceVideoOutSubmitFlip` post to every queue
rather than the registered ones - off by default, declared as intervening, so a verdict under it
is recorded as measuring a settings change. It guesses nothing about the registration; it removes
the routing, which is the one thing between a flip and that queue.

```text
baseline        UnityFTMFlipQueue (0 registered, 1 waits, 0 delivered)
FLIP_TO_ALL=1   UnityFTMFlipQueue (0 registered, 1 waits, 0 delivered)   verdict same
```

**Zero delivered anywhere, even with the routing gone.** Which means no flip is submitted after
that queue exists - and the handles agree: the two video-out queues are `0x5e2d0000b440` and
`0x5e2d0000b460`, `UnityFTMFlipQueue` is `0x5e2d0000ee40`, allocated much later. All forty-four
`sceVideoOutSubmitFlip` calls happen before the queue the guest goes on to block on was created
(D620).

**That rules out a whole line of attack.** Implementing `sceAgcDriverAddEqEvent`, working out the
identifier a post would carry, and wiring a completion to the flip path would have been days of
work against a queue nothing was going to post to in this run anyway, because the guest stops
submitting before it starts waiting.

The wall is not "orbistoun does not complete graphics work". It is that the guest reaches a state
where it waits for a frame it has not asked for. **What drives the next flip** is the question,
and it is a different one from the one four decisions have been circling.

## Next

- What drives the guest's next flip. Forty-four go out, then it waits - so the loop that would
  submit the forty-fifth is the thing to find, and it is upstream of every event queue.
- What posts to `UnityFTMFlipQueue`. Registration alone is known to be insufficient; the
  identifier a post would carry is the open question, and `arg1`/`arg2` being zero says it is not
  in the registration call.
- `image+0x39f7c`, which is now two titles' wall rather than one. The object whose `+0x30` is
  text is the question; `ReadStructure` exists for exactly this and has not been pointed at it.
- PPSA02664 reaching 215 imports on 2026-09-04 and 198 now - a regression nothing has explained.
- The remaining 8 outstanding conditions of implemented functions: `sceVideoOutRegisterBuffers`,
  `031-stackattr/self-describes`, `sceUserServiceGetUserName`.
- The 31 undeclared symbols, in the order principle 6 gives.
- The 31 undeclared symbols, in the order principle 6 gives.
- The remaining seventeen measured sections nothing interprets.
- 215 kernel exports still unnamed.
- The mapping sequence, still varying where the import count no longer does.
- The clean title's wall.
