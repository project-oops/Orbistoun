# 603. Grand Theft Auto's ceiling is an int 0x41 trap after a path it cannot resolve

**2026-09-15** - what PPSA04263 does now that worklog 601 cleared its memory wall

## The point

Clearing the direct-memory wall (worklog 601) did not open new ground - it returned PPSA04263 to
its 2026-09-07 record, `entered` at `image+0x196b91a`, 70 imports. That fault was the record's
ceiling all along, sitting behind the memory wall the whole time. This characterises it, because it
is now the title's actionable frontier and nothing had looked at it.

## It is a software interrupt orbistoun does not service

The faulting bytes at `image+0x196b91a` are `cd 41` - `int 0x41`, a software interrupt (`cd 41 eb 64
cd 41`: an int, a short jump *past* the second int, then that second int). The `read of
0xffffffffffffffff` the report leads with is how the host surfaces a trap instruction, not a real
read (D384).

**The conclusion below (the `## What this wall actually is - corrected` section) supersedes the
first reading of this worklog.** The short story: `int 0x41` is a kernel-service interrupt real
hardware handles and orbistoun does not, so the guest dies here where a retail console continues.
`app_content.rs` documents `int 0x41` as il2cpp's abort-on-error, and an early draft of this leaned
on that - but the same eboot runs on retail hardware, so here it is a service that returns, not a
fatal abort. The sections below work through how that was established.

## What was upstream

The last calls before the trap are `libc::strchr` then a run of `libc::strncmp`, all against one
buffer, mostly returning mismatches. A guest walking a list of strings and comparing each against
something it wants. Then the trap.

Two light dumps settled it, no watchpoint needed. `ORBISTOUN_DUMP` takes a named import and prints
its arguments' *contents* - one function at a time, not the per-access instrumentation principle 9
forbids on every call.

`ORBISTOUN_DUMP=sceKernelOpen,sceKernelStat` shows the guest passing **`/host//ap/rpf.cache`** as
arg0 (bytes `2f 68 6f 73 74 2f 2f 61 70 2f 72 70 66 2e 63 61 63 68 65 00`). So that path is real -
the guest's own, not this reporter's rendering, which an earlier draft of this worklog wrongly
guessed. **`/host/` is the platform's host-filesystem mount** - on a devkit it maps to the
development machine, which is how a debug build loads assets, and these dumped eboots are devkit
builds. orbistoun models `/app0` and `/data` and does **not** model `/host`, so the open fails.

`ORBISTOUN_DUMP=strncmp` shows the other half: the guest also holds `/ap/rpf.cache` in a buffer and
`strncmp`s it against `/app0/`. That is the guest classifying its own path - is this under `/app0`?
No - so it routes the cache to `/host/` instead, builds `/host/` + `/ap/rpf.cache` (the doubled
slash is that naive join), opens it, gets nothing, and traps.

So the chain, measured end to end: the guest wants its RPF cache, decides it does not live under
`/app0`, looks for it under `/host` - the devkit host mount orbistoun does not provide - and traps
when it is not there. No unimplemented import, no corrupted string: a mount orbistoun is missing.

## The correction worth keeping

This worklog said, in an intermediate draft, that `/host/` was the reporter's own prefix and the
real path was `/ap/rpf.cache`. That was wrong, and dumping `sceKernelOpen`'s actual argument is what
caught it: the guest passes the whole `/host//ap/rpf.cache`. The lesson is the methodology one the
question that prompted this raised - the always-on trace named the trap and the buffer, but the
buffer (`/ap/rpf.cache`) was not the whole path, and only dumping the *open call's own argument*
showed the `/host/` the guest actually used. Read the call that faulted, not the buffer nearest it.

Recorded rather than started, so the next session begins with the chain already drawn instead of
re-deriving that the memory wall is gone and the ceiling is a path-fed trap.

## A `/host` mount was tried, and does not move it - but the reason it does not is the real find

Adding `/host` to the manifest so the open resolves changed nothing: 70 imports,
`image+0x196b91a`, verdict `same`. The open is `O_RDONLY` (`arg1 = 0x0`), so an empty `/host` has no
cache file to read and the open still fails. Reverted (D387: a manifest entry earns its place by
*answering* a request, and this answered nothing).

**But the mount not helping is what shows the cache open was never the cause.** The wall is one
instruction further on, and it is orbistoun's.

## What this wall actually is - corrected

**This is NOT a missing-asset dead-end, and an earlier version of this worklog was wrong to call it
one.** The same eboot, byte for byte, runs on retail hardware - so orbistoun is diverging from real
hardware, and the RPF cache is not why.

The faulting instruction at `image+0x196b91a` is `cd 41` - `int 0x41`, a software interrupt - and
it is **followed by more code** (`cd 41 eb 64 ...`: the `eb 64` jumps somewhere *after* the int).
An instruction the guest expects to return from. On retail, the kernel's interrupt table services
`int 0x41` and returns; the guest continues through the `eb 64`. orbistoun has **no int-vector
handling at all** - it catches the software interrupt as an unhandled host exception and stops.

Two things rule out the alternatives:

- The guest does **not** install a signal handler (no `sceKernelInstallExceptionHandler` in the
  run), so this is not orbistoun failing to deliver a signal to a guest handler - it is a *direct*
  kernel service, entered by `int 0x41` the way a syscall gate is entered.
- orbistoun's own fault handler does not forward CPU traps to anything; it reports and exits. So any
  guest reaching `int 0x41` dies here, whatever the guest.

So the wall is an **OS-level gap: orbistoun does not implement the `int 0x41` kernel-service
interrupt** the platform provides. It is orbistoun's to fix, it is general rather than
GTA-specific, and it is exactly the "we go under the library boundary and there is no kernel" case
D378 anticipated - reached now by a commercial title rather than a payload.

## What it needs, and the honest uncertainty

To service `int 0x41`, orbistoun needs to know what it *does*: which register carries the service
number, what it reads, what it returns. That is either measured (obSCEne can execute `int 0x41`
under known register state and observe) or referenced from an open-source PS4/PS5 kernel
reimplementation, which will have solved this already.

The residual uncertainty is whether `int 0x41` here is on GTA's *normal* path (a service it always
calls, which orbistoun must implement) or an *error* path (an abort it reaches only because
orbistoun fed it a wrong value upstream - `int 0x41` is also il2cpp's abort-on-error in
`app_content.rs`). The `jmp`-after-the-int and the retail evidence favour the former, but settling
it is the next step: single-step to the int with the registers, and see whether the service-number
register names a routine that returns or a fatal abort.

Either way the earlier conclusion - "not orbistoun's to fix" - was wrong, and is retracted here.

## Gate state

No code changed; this is a characterisation. `./bin/orbistoun worklogs` unique, identity scan clean.
