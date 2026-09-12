# D680 - the app-content init sequence, and that it is not Terminator's wall

**Status:** decided
**Date:** 2026-09-12

## The choice

Implement two `libSceAppContent` calls a Unity IL2CPP title runs at startup -
`sceAppContentInitialize` (answers `0`) and `sceAppContentAppParamGetInt` (writes the documented
placeholder `0`, answers `0`) - and record that doing so does **not** move PPSA25872's wall, because
that wall is a separate assert.

## Why implement them

PPSA25872 (Terminator 2D) is Unity IL2CPP. `libil2cpp` prints its own failures:
`[libil2cpp] sceAppContentInitialize returned 0xf7ff0001`, then `sceAppContentAppParamGetInt failed
0xf7ff0001`. These are the unimplemented placeholder reaching guest-visible behaviour. Answering them:

- `sceAppContentInitialize -> 0`: guest-observed, the same shape as `sceCommonDialogInitialize` (D678).
  obSCEne's `130-layout/app-content` resolved it to `0x0`, and the guest oracle proceeds (FURTHER). The
  boot out-parameter is left unwritten - the guest does not fault reading it and nothing measured says
  what it holds.
- `sceAppContentAppParamGetInt -> 0` with `0` written to the out-pointer: the documented placeholder,
  identical to the shipped `sceSystemServiceParamGetInt` - the app-param identifiers are title metadata
  nothing has measured, and `0` is the least-surprising default. Honest as a placeholder, not a measured
  value.

Both clear a real guest-logged failure and are the correct behaviour for any Unity title's app-content
init. This is the same class of implementation as common-dialog, not the guess-based APR fallback (D679)
that was reverted - these are exercised, guest-observed/placeholder-by-a-shipped-pattern, and active.

## What it does not do, and why that is recorded

It does **not** clear Terminator's wall. The `int 0x41` assert at `image+0x17554a3` fires at the same
point (~321,962 calls) on the same fixed path (`0x1755b55 <- 0x7b6081 <- ...`) before and after these
implementations. The app-content log lines are **warnings `libil2cpp` continues past**, not the assert
cause - the first diagnosis (worklog 519 had already corrected the APR misattribution; this corrects a
second one: app-content is not the wall either). The next unimplemented app-content call
(`sceAppContentTemporaryDataMount2`) is a further warning, not the wall.

So Terminator's real wall stands unidentified: a fixed guest assert (`test byte [rax+0x30],0x10; je;
int 0x41`) preceded by error-message formatting, with another thread in `sceKernelSyncOnAddressWait` at
the moment of the trap - which points at a threading or fatal-error path, not a missing init function.
That is the next Terminator investigation, and it is not "implement the next app-content call".

## Consequence

The two handlers stay (they are correct HLE, and benefit any Unity title). Terminator's frontier is
unchanged by them. The honest record is: two guest-logged app-content failures fixed; the int-0x41 wall
is separate and open. No more app-content calls are implemented on spec - the wall is not there.
