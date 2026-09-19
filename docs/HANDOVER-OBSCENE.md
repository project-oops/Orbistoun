# What orbistoun wants measured next

The reverse of obSCEne's `HANDOVER-ORBISTOUN.md`: findings and requests going the other way, so
neither side has to rediscover what the other already knows.

**Status: written 2026-08-30, after absorbing `data/hardware/ps5-full.txt`.** That run settled
seven things this project had been guessing at, and D398 lists them. This is the residue - what
it did not answer, and what it accidentally showed was worth asking.

Each item says what to call, what to record, and **what it unblocks here**, because a probe that
costs a hardware run deserves to be aimed at something.

---

## 1. `sceKernelGetModuleInfo` - it failed, and the failure is informative

The run recorded `110-modules/names` as `fail 0x80020016`, described as *the platform would not
describe any module*.

**It is worth re-reading that as a probe result rather than a platform answer.** `0x80020016` is
the invalid-argument errno. The console refused the *call*, not the request - and the same run
shows it happily answered `sceKernelGetModuleList` with 31 modules, so it is not that modules
are undescribable.

The overwhelmingly likely cause is the one every structure-out call in this family has: **the
caller has to write the structure's own size into it before the call**, so the kernel knows
which layout the caller compiled against. Passing a zeroed buffer gets exactly this code.

**What to try.** Sweep a candidate size into the first field and call again for each:

```
for size in [0x100, 0x110, 0x120, 0x130, 0x140, 0x150, 0x160, 0x1a8, 0x200, 0x280, 0x300]:
    zero the buffer; write `size` as a 64-bit value at offset 0; call; record the code
```

Record the accepted size and then **dump the whole buffer** the way
`130-layout/system-software-version` already dumps one - `extent`, `changed`, and the bytes.

**What it unblocks.** D395 stopped short of inventing this structure and said the layout had to
be measured rather than guessed. orbistoun currently refuses the call honestly and cannot
describe a module to a guest. One accepted size plus one dump finishes it, and the dump is worth
more than the size: the module *names* are in there, and a title that enumerates modules is
looking for one by name.

---

## 2. `sysctlbyname` - nothing in the run asks it anything

There is no sysctl record of any kind in the capture, and this is now the single most valuable
gap.

**What to ask for.** Each as a string or integer, whichever the name implies, recording the
returned bytes and the length:

| name | why orbistoun wants it |
|---|---|
| `kern.osrelease` | **The one that is blocking something today.** See below. |
| `kern.ostype` | Says whether the kernel identifies as its upstream at all |
| `kern.version` | The long form, which usually carries a build date and configuration |
| `hw.ncpu` | orbistoun invents a core count |
| `hw.pagesize` | Assumed rather than measured |
| `hw.physmem` | A second opinion on memory size, from a different subsystem |
| `machdep.tsc_freq` | A **third** route to the counter frequency, which is worth having |

**What it unblocks.** D397 is stuck on exactly one value. `zftpd` calls `sysctlbyname` precisely
once, for `kern.osrelease`, and reports `Firmware detection failed` and turns a feature off when
it does not like the answer. orbistoun refuses the name rather than inventing a version, because
a plausible answer would send the guest down a path chosen by a number nobody measured - so the
setting sits empty waiting for this.

**A hypothesis worth disproving while you are there.** The same run read
`sceKernelGetSystemSwVersion` and got the string `13.090.001` with a packed `0x13090001`
alongside it. It is tempting to assume `kern.osrelease` says something similar. It has not been
assumed here, because they are different calls answered by different layers and substituting one
for the other is the exact move this project refuses. If they turn out to match, that is a
finding; if they do not, it is a bigger one.

---

## 3. The third field of the memory-query structure

The run read `3` from it for the region at the bottom of the map. orbistoun had a boolean there
- whether the span was taken - and no boolean is ever `3`, so the previous meaning was provably
wrong. It now carries a memory type. **Whether `3` denotes a type or some state is still open**,
and one run separates them.

**What to try.** Allocate several spans with different memory types, then query each back and
record field 2 for each:

```
for type in [0, 1, 2, 3, 4, 5, 6]:
    allocate a span with that type; query its offset; record the third field
```

If field 2 tracks the type asked for, it is a type. If it stays constant while the type varies,
it is state and the type lives somewhere else - in which case dump the whole 24 bytes for two
regions in different states and diff them.

**What it unblocks.** This is the most-called function in every commercial executable examined -
99.9% of guest calls, one title reaching four hundred million in ten seconds. Every field of its
answer is worth knowing exactly.

---

## 4. Does a short query buffer truncate, or overrun?

The run swept the declared size from 1 to 256 and recorded every one as **accepted**, which was
genuinely surprising - orbistoun had been refusing anything smaller than the whole structure on
its own reasoning, and that refusal is now gone.

But *accepted* was measured from the return code, and the return code cannot say how many bytes
were written. orbistoun caps the write at what the caller declared, on the grounds that of the
two guesses, overrunning a buffer the guest sized is the one that cannot be undone. **That is a
guess and it is marked as one.**

**What to try.** The pattern already used in `018-relational/handle-fits-its-out-parameter`: put
a known guard word immediately after a deliberately short buffer, call with the short size, and
report whether the guard survived.

**What it unblocks.** It is the difference between a cap that matches the platform and a cap
that silently hides data a guest was expecting.

---

## 5. Where `GetProcessTime` counts from

The run measured this clock's **unit** conclusively - a 20000us sleep advanced it by 0x4fbb,
which no other unit produces - and that assumption is now retired.

Its **origin** is still open, because the run recorded deltas and a delta is blind to where
zero is.

**What to try.** One absolute reading as early in the run as possible, recorded raw. A small
number means it counts from process start; a very large one means an epoch. That is the whole
measurement.

**What it unblocks.** A title comparing this against a stored timestamp behaves completely
differently under the two, and nothing in a trace would say which.

---

## 6. Module handles, and the `0x2001` that has been haunting this project

The run recorded something it was not looking for. `sceKernelLoadStartModule` returned:

| asked for | returned |
|---|---|
| a system path | `0x80020002` - no such entry |
| `/app0` libc | `0x15` |
| `/app0` fios2 | `0x14` |
| libkernel | **`0x2001`** |

**`0x2001` is a constant orbistoun has been chasing for a while.** Two payloads, `elfldr` and
`pldmgr`, die early with exactly that value, and it survived every variation of the handoff it
was given - which is how it was established to be a constant rather than something derived. It
was never identified. This run says it is a **module handle for a system module**, which
reframes those failures completely: they are not failing *with an error*, they are getting a
handle back and doing something with it.

**What to try.** Load several system modules by name and record each returned handle. If system
modules occupy a distinct numeric range from `/app0` ones - small integers for the application,
`0x2000`-and-up for the system - then the range is the finding, and orbistoun should hand out
handles from the same two ranges.

**What it unblocks.** Two of the five core payloads, and the handle-numbering scheme underneath
them.

---

## 7. Cheap things worth adding while a run is happening

- **Read the counter frequency twice**, far apart in the run. It is treated here as fixed; one
  pair of readings confirms it does not shift under load or thermal state.
- **`sceKernelGetAvailableFlexibleMemorySize`.** orbistoun answers it from the same pool as
  direct memory, because a second figure nothing subtracted from would drift away from the
  first. If it is genuinely a separate budget, that is wrong and worth knowing.
- **`sceKernelDlsym` against a symbol that exists.** The run asked for `memcpy` and got
  `0x80020003`. That may mean the symbol is absent from that module, or that the call needs
  something else first - a second symbol known to be present separates the two.

---

## What came back the other way

For completeness, since this file is the record of the exchange: the run settled the vendor error
encoding, the direct memory size, the counter frequency, the microsecond unit, the accepted
query flags and sizes, the default mutex attribute type, and the type-dependent behaviour of
`trylock`. All of it is recorded against the functions it belongs to with the run cited, and
D398 has the detail.

The single most useful property of that capture was not any one value - it was that the run
**cross-checked itself**. The frequency the console reported and the frequency implied by its own
sleep test agree to four significant figures, which turns one reported number into two
independent observations. More of that, wherever it is cheap.

---

## Update 2026-08-30: the module probes ran, and one had a bug

The `110-modules/handles` and `110-modules/info-size` sections ran on a 12.40 console and the
results are in `data/hardware/ps5-imports.txt`.

**`handles` delivered:** 32 module handles, `0x0, 0x2001, 0x2, 0x11 ...`. `0x2001` is confirmed
as a live handle (libkernel's), which settles the constant two payloads were dying on.

**`info-size` did not, and the reason was a probe bug:** it tested `GetModuleInfo` against
`handle[0]`, which is `0x0`, and the null handle refuses with EINVAL no matter the size - so
every rung of the ladder rejected and the layout was never actually tested. Fixed now:
`obs_first_describable_handle()` picks the first non-zero, non-`0x2001` handle, and the ladder
and the naming check both use it. **Re-run these two on hardware** and the `GetModuleInfo` layout
D395 refused to guess should finally fall out - the struct size the console accepts, and a dump
of what it writes.

---

## Update 2026-08-31: the handoff, measured whole, and the kernel base with it

`136-kernel/handoff` ran on a 12.40 console as a real elfldr payload and read the whole
`payload_args` (orbistoun D408). Two obSCEne bugs were fixed to get it there and are worth
keeping: a payload build resolves no imports, so `sceKernelWrite` was null and the report went
nowhere until the output channel was bootstrapped from `payload_args[0]=getpid`; and the check
was gated on that same unresolvable import, so it skipped in the one context it exists for.

**The measured struct:** word 0 getpid (`0x8000005b0`, confirming the base); words 1, 2, 5
userland pointers; word 3 a kernel-heap pointer; word 4 `0xffffffff8c290000`, the **kernel base**;
6-19 null. The kernel base is the anchor a ucred-offset walk starts from, and it is measured now.

**What is wanted next, and it is a probe, not a wall.** The escape needs the 12.40 kernel offsets
a ucred patch turns on. With the primitives elfldr hands over (the kernel-heap pointer and the
kernel base), a second `136-kernel` check can *use* the raw read to walk kernel structures from
`0xffffffff8c290000` and report the offsets - the un-safe half the recon probe was gated ahead of.
That is the measurement orbistoun needs to model or no-op the escape and let a payload reach
`main`. It reads kernel memory, so it must guard every access and stay behind the recon check's
"a kernel context is present" pass.

---

## Update 2026-09-04: ps5_mode is cracked and Agc is reachable - the probe set

The operator reports access to libSceAgc. This is the ask, aimed and prioritised from
**measurement rather than guesswork**: every function below is one PPSA02664 actually calls, the
counts are its counts, and the argument classes were read off the guest's own calls in a run made
for this purpose (D559).

### First, the safety gate - because D008 blocks the obvious plan

D008 forbids calling a function whose arity is uncertain, and obSCEne's standing for Gnm is D107:
two independent open reimplementations agreeing. **That standard cannot be met for Agc.** shadPS4
and GPCS4 are PS4/Gnm projects; there is no mature open Agc reimplementation to agree with
anything. Taken literally the gate blocks the entire ask.

It can be met a different way. **In System V AMD64 the first six integer arguments are in
registers, and a callee simply ignores register arguments it does not take.** A call that sets all
six to individually safe values is therefore safe for *any* arity up to six, whatever it turns out
to be - the stack is never involved, which is the thing D008 is protecting. What remains is not
arity risk but **value** risk: a pointer parameter handed a non-pointer, or a size parameter
handed a huge number.

That is what the argument census below is for, and it is why this ask leads with which functions
are safe rather than with which are interesting.

### The four classes, read off the guest

Every Agc function PPSA02664 calls, grouped by what it passes in `arg0`. The three distinct
pointer values are the finding: functions sharing one are taking the same object.

**Class B - a caller-owned writer struct on the stack. Start here.**

`arg0 = 0x6000007fbe38`, and the guest's stack there holds
`{begin = 0x6000007fbe70, end = 0x6000007fc270, begin, end}` - a **0x400-byte command buffer with
a begin/end pair in front of it**, the struct sitting 0x38 below the buffer it describes.

| calls | function |
|--:|---|
| 2 | `sceAgcCbNop` |
| 2 | `sceAgcCbReleaseMem` |
| 2 | `sceAgcDcbDmaData` |
| 2 | `sceAgcDcbWaitRegMem` |
| 2 | `0x7d86501b8094ef57` (unnamed; takes `arg0 - 8`, where a count `0x1fa` sits) |

**These five are constructible from nothing** - a probe allocates a buffer, writes a begin/end
pair in front of it, and calls. This is exactly `gnm.c`'s technique and the same safety class.

**Begin with `sceAgcCbNop`.** A no-op's packet is the simplest encoding that exists, so if nothing
appears in the buffer the instrument is wrong rather than the platform - the same role `pipeline`
plays in the GPU section.

**Class A - a library-owned Dcb handle.** `arg0 = 0x740002447868`, one heap object shared by nine
functions:

| calls | function |
|--:|---|
| 5 | `sceAgcDcbEventWrite` |
| 4 | `sceAgcDcbPushMarker` |
| 3 | `sceAgcDcbPopMarker` |
| 2 | `sceAgcDcbAcquireMem` |
| 2 | `sceAgcDcbResetQueue` |
| 1 | `sceAgcDcbSetCxRegistersIndirect` |
| 1 | `sceAgcDcbSetUcRegistersIndirect` |
| 1 | `sceAgcDcbSetIndexSize` |
| 1 | `sceAgcDcbWaitUntilSafeForRendering` |

**The open question that unlocks all nine: what creates that object?** Nothing orbistoun has
observed constructs it - the guest already holds it by the first call. If the census (below) names
a constructor, that one call opens nine.

**Class C - a handle returned by an earlier call. Not standalone-probeable, and this is why it
matters.** Four of these receive `0x7fff0001` in `arg0` - which is **orbistoun's own placeholder**,
meaning the guest fed them the return value of a call nothing implements:

`sceAgcSetCxRegIndirectPatchAddRegisters` (23 calls), `sceAgcSetUcRegIndirectPatchAddRegisters`,
`sceAgcSetCxRegIndirectPatchSetAddress`, `sceAgcSetUcRegIndirectPatchSetAddress`.

Three more take `0x140081c3bf21` - unaligned, in a region unlike any host pointer, so a packed or
tagged handle rather than an address: `sceAgcQueueEndOfPipeActionPatchAddress`,
`sceAgcWaitRegMemPatchAddress`, `sceAgcDmaDataPatchSetDstAddressOrOffset`.

**Do not call these blind.** Their first argument is a handle whose producer is unidentified, and
a fabricated one is exactly the value risk the safety argument above does not cover. They come
free once Class A and B are answered.

**Class D** - `0x53bbd82b51d172db` (1 call) takes a pointer to a static descriptor in the guest's
own image, `00 00 00 00 01 00 00 00` then zeroes. A probe can pass a similar zeroed block.

### What to record for each call

The `gnm.c` shape, unchanged: poison the buffer, call, dump the delta and the return value. The
bytes it wrote **are** the finding - that encoding is what orbistoun's command processor must
parse, and there is no other way here to learn it. A guard band behind the buffer catches a write
that runs long, which is the one fault this could cause and not otherwise notice.

Two passes are worth it where a builder writes nothing for zero: `arg1..arg5 = 0` first, then
small plausible values (1, 2, 4), one register at a time.

### The shader object - the actual wall

`sceAgcCreateShader(out, header, bytecode, arg3)`, arity 4 from dumps (D194).

**Record:** the 32 bytes at `out`; then, if `*out` is a mapped pointer, **0x200 bytes from it**.
Twice, with two different payload lengths, so fields that vary separate from fields that do not.

Orbistoun supplies the header shape, already established from the guest: it begins `31 32 33 34`
(`'1234'`), then `0x18`, then a length - `0xd8` and `0x118` both observed.

**What it unblocks:** the guest reads a dword at **+0x50** and keeps only its low byte, then a
quadword at **+0x30**. Those two offsets are the wall, and a run with a 4KB region planted there
takes PPSA02664 from 197 imports to 215 and from 1 shader created to 33 (D559). Measured
provenance also means orbistoun records it as an **honest** run rather than an experiment (D557),
which a guessed layout never can.

### The surface census, worth simply re-running

472 libSceAgc symbols are recorded `absent` across the existing captures, and the reason is in the
record: *"excluded at build time: known to end the process on this platform"*. If the library now
maps, **re-running the existing census converts all 472 into a real surface** with no new code.

Two NIDs the guest calls are named by nothing on either side - `0x53bbd82b51d172db` and
`0x7d86501b8094ef57`. Neither matches any of the 111 Agc names obSCEne knows; orbistoun swept all
111 against both hashes and got no hit. Any new name the census reports, orbistoun can hash and
place.

**`0x7d86501b8094ef57` is settled (obSCEne `-7c21`, resolved 2026-09-17):** it is **not** an export
of retail `libSceAgc.sprx` on FW 12.40 - runtime resolution fails (`skip`) across three sweeps
(`20260917-124503`, `-143259`, `-160206`) - and a forward-hash search of 834,780 mined words produces
it in neither byte order. So **no citable vendor name exists**; it is an unexported internal/private
linkage. Orbistoun should stop treating it as a nameable hash: if a handler is ever wanted (it sits in
the loop at the leading titles' `0xa8` wall, call shape `(ptr, ptr, 0, ptr, u32, u32)` with the
command buffer in `arg1`), it must be dispatched by NID, not by a name that will never arrive.
`0x53bbd82b51d172db` remains open.

### Free while the console is up

Orbistoun's ask list is **790 open questions across 502 functions**, but only **143 distinct
premises**, and 28 of those carry 629 of the total. `orbistoun-cli questions --premises` ranks them
by how much a single answer retires - which is the right order to spend a hardware day in.

---

## Update 2026-09-04, second: the Dcb "constructor" is the wrong question

`run-native-title.txt` came back and Class B worked - four builders called, four encodings
captured, and orbistoun's packet walker consumes three of them exactly (D565). Thank you. Two
things follow, one a correction to the ask itself.

### The correction: those nine functions are Class B, not Class A

The first handover asked you to **find what constructs the Dcb object** those nine functions take,
calling it "a library-owned handle". **That was wrong, and it was orbistoun's mistake to make.**

`0x740002447868` is not a handle libSceAgc issued. `0x7400_0000_0000` is orbistoun's own fixed-base
heap - it is in `docs/ADDRESS_MAP.md`, which is gated against the source so it cannot go stale, and
was not consulted. The address is **memory the guest allocated itself**.

So there is probably no allocating constructor to find. There is an **initialiser taking a
caller-allocated block**, which is the Class B shape you have already proved four times.

### The one call worth making

**`sceAgcDcbResetQueue`.**

PPSA02664 calls it on **that exact pointer**, twice, before every other use of the object. "Reset
queue" on a freshly allocated block is what an initialiser looks like, and it is the only observed
call that could be one.

The recipe is the one that already worked - no new technique:

- allocate a block, poison it, pass it as `arg0`
- `arg1..arg5 = 0` first, then a second pass with a plausible size in `arg1` (the guest's buffer
  work elsewhere is 0x400-sized, so `0x400` is a reasonable second value)
- **dump the delta and the return value**

**What it answers:** if it writes a structure, that structure is the Dcb, and nine functions open
at once - `EventWrite`, `PushMarker`, `PopMarker`, `AcquireMem`, `SetCxRegistersIndirect`,
`SetUcRegistersIndirect`, `SetIndexSize`, `WaitUntilSafeForRendering` and `ResetQueue` itself. If
it writes nothing and returns an error, that is equally useful: it says the object is built
somewhere orbistoun has not seen, and the search moves elsewhere.

### And a bug worth more than the ask

**The report contradicts itself.** It records `sceAgcCbNop` as `absent`, along with 121 other
libSceAgc symbols - while section `166-agc` **called that symbol and captured four bytes of its
output**. All five symbols the Agc section exercised are recorded absent.

The cause looks like `900-surface/agc`, which **skipped** with *"belongs to the other console
generation, so absence is expected rather than a gap"* - on hardware `005-generation` had just
identified as prospero with `gpu = agc`. The gate appears inverted for this library.

**This matters to orbistoun specifically**: the first handover was built on *"472 libSceAgc symbols
recorded absent"* and treated that as the blocker on the whole Agc axis. It was not measuring what
it appeared to. Fixing it is probably also the cheapest way to answer the constructor question
properly - a working census of libSceAgc's exports would show whether any constructor-shaped name
exists at all, which no amount of calling can.
