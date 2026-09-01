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
