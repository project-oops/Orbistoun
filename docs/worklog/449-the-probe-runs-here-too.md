# 449. The probe runs here too

**2026-09-08** - directed, continuing 448

## The premise four decisions rested on was wrong

D613, D615 and D620 all treated a thread blocked in `sceKernelWaitEqueue` as the wall in the two
Unity titles, because the run report prints the fault and the recent calls together. **The call
tail is every thread's and the fault is one thread's**, and nothing said which lines belonged to
the thread that died.

They were different threads. With the fault naming its own:

```text
! the guest faulted at image+0x39f7c, ..., on guest thread 0x5e2d00000a20
    just before: libSceAgc::sceAgcCreateShader(0x6000007fc538) -> 0x7fff0001
    just before: libc::memcpy(0x74000edc0200)
    just before: libc::memcpy(0x74000edc0100)
    just before: libkernel::scePthreadSelf(...) -> 0x5e2d00000a20
    just before: libkernel::sceKernelWaitEqueue(0x5e2d0000ee40)  [another thread]
```

`scePthreadSelf` on the faulting thread answers the handle the fault reports - the join confirmed
from both ends. **The wall is `sceAgcCreateShader` answering `0x7fff0001`**, which is still in
`rbx` three calls later. It was one line below where anybody was looking (D621).

Same shape as D616, which found `last_call` answering "what am I inside" with another thread's
call. Two questions answered by one function, because single-threaded they have the same answer.
Third time this session the finding has been a *missing distinction* rather than a wrong value.

## The probe runs under orbistoun

Its payload build executes here and emits the same `OBS|` stream the hardware reports carry. So
the same binary runs in both places and its own verdicts can be compared - and a check that passes
on a console and fails here is a **named, sourced defect with the probe's own sentence attached**.

`orbistoun-cli probe <local> --against <hardware>`:

```text
256 of 635 check(s) both ran concluded differently
115 passed there and failed here - each is a defect with its own words
  020-memory/allocate            allocation was refused
  020-memory/virtual-query-text  virtual query on code address refused
  050-time/usleep                a short sleep was refused
  120-measure/sleep-fidelity     a sleep returned far sooner than asked
  102-net/sockaddr-bind          bind refused both sockaddr lengths
  900-surface/control            a symbol that does not exist reported present;
                                 every count in this section is meaningless
```

Only checks both sides ran are compared - one side skipping is a difference in reach, not
behaviour, and listing it would bury the ones that are (D622).

`900-surface/control` is the probe reporting `sceKernelDlsym`'s known divergence **and its blast
radius**: every count in that section is meaningless because of it. No wall has ever said that.

`titles/obscene-payload` is in the corpus now: **223 distinct imports, 100% standing, exits
cleanly** - more of the interface than any title, and the only guest that terminates rather than
crashing or timing out.

## Surprises

- **Today's `eboot.bin` does not load**: `no usable dynamic table`. The corpus copy is a week old
  and does. Five program headers against six, two vendor segments against one, an SDK version
  where there was none, and null proc-param pointers. The corpus has been measuring a week-old
  probe with nothing saying so.
- **Every record in a local transcript appears twice**, and the first counts I took from it were
  therefore doubled. Not a defect on either side: obSCEne writes each line to descriptor 1 *and*
  descriptor 2 on purpose - *"one emulator's stdout is not the handle a parent process redirects,
  so the same line goes to stderr as well"* - and orbistoun sends both to the host's error stream
  because its **standard output carries the worker protocol**. Both are right and the result is
  a doubled stream.

  The differential is unaffected, because `verdicts` keys by check. Raw record counts are not:
  the payload run's real figures are 162 pass and 378 fail, not the 324 and 756 I first read.
- Orbistoun *passes more checks than the console does* on the payload run - 162 against 67 -
  because the payload has limited reach on hardware. The comparison that means something is
  against the console's **pkg** run, which is the one with full access.
- A batch of reports arrived, and another, and another, during this iteration. The measurement
  table now carries all of them because D618 taught it to accumulate.

## The dump answered a different question

Chasing `020-memory/allocate` - "allocation was refused" - the implementation turned out to be
innocent. Called directly with obSCEne's own arguments (`0, 0x140000000, 64 KiB, 16 KiB, 0, &out`)
it answers `0x0` and hands back `0x10000`. So the refusal is between the probe and the
implementation, not in it.

Reaching for `ORBISTOUN_DUMP` to see the real call found two tool defects first:

```text
ORBISTOUN_DUMP=sceKernelWaitEqueue   on PPSA03416   -> dumped
ORBISTOUN_DUMP=sceKernelAllocateDirectMemory  on the payload -> a list without it in
```

**A forced list was added to the default set rather than replacing it**, and the default set -
every import nothing implements - filled the 512-entry buffer with the payload's unimplemented
maths library long before the import somebody had actually asked about. A forced list now narrows
the dump to itself.

**And a dropped dump printed as nothing at all**, so the tool answered and omitted what it was
asked for without saying so. Counted and reported now:

```text
orbistoun: 45 argument dump(s) wanted after the buffer was full -
           name an import with ORBISTOUN_DUMP to spend the room on it
```

My first reading of this was wrong and is worth recording as wrong: I concluded "every diagnostic
keyed on an import label is inert for a bare-ELF guest", which is not true - the labels resolve
fine. It was a buffer, and the reason I believed the stronger claim is that a silent drop and a
guest that never calls the thing look identical from outside (D623).

With the forced dump given the whole buffer, `sceKernelAllocateDirectMemory` still produces
nothing - **which now means something**: the payload never reaches a thunk for it. That is the
memory question properly posed for the first time.

## Next

- Where `sceKernelAllocateDirectMemory` goes instead of a thunk. The run resolves it to a by-name
  stub at `0x700000006980`; whether that stub and the import-table entry share an index is the
  thing to establish.
- The memory group: one refused allocation cascades into six skipped checks.
- The sleep group, which sits next to D614's early-timeout fix in a way that wants looking at.
- Why today's `eboot.bin` container reads as having no dynamic table.
- `sceAgcCreateShader`, which is the actual wall and needs a hardware answer for its return.
