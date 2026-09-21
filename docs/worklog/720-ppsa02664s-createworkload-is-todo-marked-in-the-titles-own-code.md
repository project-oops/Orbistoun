# 720. PPSA02664's CreateWorkload: TODO marker in the guest, but the title runs on hardware

**2026-09-19** — continuing the accurate investigation of PPSA02664 (Alex Kidd in Miracle World)'s
wall. Setting up to disassemble the faulting call site turned up a `todo:` marker in the guest's own
output, which led to a debug-build inference below - **overturned by the operator: this title runs on
real hardware** (see the correction). The observations stand; the conclusion drawn from them was
wrong, and the correction is the load-bearing part of this entry.

## Correction (operator, 2026-09-19): the title works on hardware

PPSA02664 renders on a real PS5. That refutes the "debug/incomplete build" reading below: the `todo:`
lines are **leftover logging**, `CreateWorkload` **completes on retail**, and the null orbistoun dies
on is a **real emulation gap**, not the title failing to finish its own code. The wall is
orbistoun's to close.

That revives a genuine tension to resolve, not paper over:

- The title reaches a render on hardware, so on hardware `CreateWorkload` does **not** die here.
- obSCEne's census (`-7c21`, thorough: 472 symbols, forward-hash over 834,780 names) says
  `0x7d86501b8094ef57` is **not** a `libSceAgc` export on FW 12.40, so on hardware an ordinary import
  of it would bind NULL - the same null orbistoun sees.

Both cannot be true of the *same* code path. The leading reading, given obSCEne's census is trusted,
is that **on hardware the guest does not call `0x7d86501b8094ef57` at all** - the branch that reaches
it is not taken there - and orbistoun reaches it because of an **upstream divergence**: something
orbistoun answers earlier steers Unity into a `CreateWorkload` sub-path a real machine skips. The
alternative is that the symbol resolves on the title's platform by a route obSCEne's synthetic probe
did not reproduce (a providing module the real title loads). Either is orbistoun-relevant and
neither is "the title is broken".

**Revised next step:** stop treating the phantom as the thing to implement, and hunt the divergence -
what orbistoun returns in the calls leading into `CreateWorkload` that a real machine returns
differently, sending the guest down the branch that reaches the unresolvable call. The guest-oracle
reconstruction (worklog 719) stays as the fallback for the case where the call *is* taken on hardware.

---

*The original observations and the (overturned) debug-build inference follow, kept for the record.*

## The guest prints its own "todo:" for the faulting function

A plain run's stderr, in order, immediately before the fault:

```
TODO:static void LocalFileSystemPS5::SetupArchive(const char *, const char *)
...
TODO:virtual bool LocalFileSystemPS5::Enumerate(const char *, FileEntryInfoList *, ...)
todo: void GfxDevicePS5SharedData::CreateWorkload()
orbistoun: guest fault: read of 0xa8 while executing at 0x7ff9c071dc8d (inside libc::memcpy)
```

Those `TODO:`/`todo:` lines are the **guest's** prints, not orbistoun's - confirmed by grep:
orbistoun's source contains none of `todo: `, `GfxDevice`, `CreateWorkload`, `LocalFileSystemPS5`
or `SetupArchive`, and it deals in NIDs and symbols, never a C++ method signature like
`GfxDevicePS5SharedData::CreateWorkload()`. So Unity's PS5 backend in this build **marks
`CreateWorkload` as a TODO** and prints that marker as it enters, then faults inside it - exactly as
its `LocalFileSystemPS5` methods, which are demonstrably unfinished stubs, announce themselves.

## Why this reframes the wall

Put beside what was already established, the picture is coherent:

- `CreateWorkload` calls `libSceAgc::0x7d86501b8094ef57`, which obSCEne confirmed is **not a retail
  `libSceAgc` export** - it binds NULL on retail (`-7c21`/`-e245`).
- A retail Unity build **inlines** the AGC SDK's small helpers, so it emits no import for them and
  never faults. A build that emits `0x7d86501b8094ef57` as an out-of-line import is one where that
  inlining did not happen - a debug / non-release / early-port build. The `todo:` markers on three
  PS5-backend methods point the same way.

So the null the run dies on is **this build's own unfinished graphics path** reaching for a helper
that, in a shipping build, would be compiled into the caller. It is not a platform API orbistoun
declined to implement; it is scaffolding the title itself did not finish, calling a symbol that does
not exist as an export anywhere.

**Consequence for hardening.** No amount of accurate orbistoun emulation moves this wall, because
the thing that is missing is not on orbistoun's side of the boundary:

- If `0x7d86501b8094ef57` is a real SDK inline, its body is the unobtainable layout worklogs
  716-719 established cannot be named or measured - and implementing it means inventing it (the hack
  ruled out).
- If it is dev-only scaffolding, it has no retail behaviour to be faithful *to* at all.

Either branch ends the same way: PPSA02664, in the build the corpus holds, has a wall that is a
property of the build, not an emulation gap. This is the honest ceiling for this title, and it is
why the guest-oracle probing (worklog 719) found the producer's args and return had no bearing on
the fault - there is no caller-supplied state that fills a phantom.

## What would confirm or overturn it

A release build of the same title would settle it: if the out-of-line `0x7d86501b8094ef57` import
disappears (inlined) and `CreateWorkload` runs without the `todo:` marker, the wall was the build;
if a release build still imports it, it is a real retail helper and the layout question stands
(still unobtainable). Neither build is in hand, and obtaining one is out of scope.

## Recommendation

PPSA02664 is a poor target for *reach* hardening: its own graphics backend is TODO-marked and
reaches for a phantom. Accurate-fidelity work on the AGC APIs it exercises before the wall (the
reservation-skeleton builders) remains valid and is unaffected by this, but it will not move the
run further. A title without these debug-build artifacts is where reach hardening pays off.

## Gate state

No code changed - a run, a reading of the guest's own output, and a grep of orbistoun's source. A
disassembly pipeline (Docker + capstone + ELF program-header parsing) was built and confirmed the
eboot's segments are plain, but the faulting code is in a runtime mapping (`0x74…`), not a
statically-placed module, so static disassembly does not reach it; the guest's own `todo:` print
answered the question first. Identity scan clean.
