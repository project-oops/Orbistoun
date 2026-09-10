# D659 - Two walls that hardware cannot reach

**Status:** measured
**Date:** 2026-09-09

## The answer was terminal, and that is worth as much as a measurement

`REQ-20260909T2145Z-9e52` asked obSCEne for the return contracts of the four calls PPSA25872 makes
that nothing implements. The answer is `not-possible`, across every leg:

| leg | why |
|---|---|
| payload | loading `libSceUserService` / `libSceAppContent` / `libSceCommonDialog` as an unsigned payload trips a kernel privilege signal |
| pkg | the OS refuses PS5 title sysmodules inside a compatibility container |
| eboot | those retail libraries are unlinked in a homebrew container |

So orbistoun now has **two** walls no probe can measure: `sceAgcCreateShader` and this family. Both
are retail-title-only, and both are blocked by the same thing - there is no execution leg on that
console where a native title's libraries are mapped. `REQ-20260909T1310Z-c8b5` asks Prosperous for
exactly that leg, and it is now the common unblock for both rather than a graphics-only question.

The instruction in the acceptance line was "tells this project to stop waiting on them", and that
is the value: four functions leave the queue.

## Which leaves the guest as the only oracle, and the guest now talks

CLAUDE.md lists the guest as the third oracle - *"a 1-bit oracle per call site: return `Ok`, does
it proceed?"* - and D226 records why that has always been weak: an intervention that moves a wall
is not a diagnosis, because nothing says what the guest did with the answer.

D658 changes the arithmetic. The guest logs its own boot, so an intervention now has a **second
observation of a different kind** attached to it for free: not "the fault moved" but "the engine
stopped saying the thing it used to say".

## The first result is negative, and legible because of that

`libSceAgc::0x53bbd82b51d172db` is `sceAgcInit` (D649, attributed not derived). PPSA02664 calls it
once, inside graphics initialisation. Forced to `0`:

```text
unforced   198 imports   417,667 calls   image+0x39f7c   … reaches GfxDevicePS5SharedData::CreateWorkload()
forced 0   153 imports   410,821 calls   image+0x3ac99   … stops after /app0/Media
```

**Zero is not the answer.** The guest gets *less* far with it than with orbistoun's
`Unimplemented` placeholder, and the log says where it diverged - before video-out setup, rather
than at some address requiring interpretation. Three unforced runs agree at 198 and the same fault
site, so the difference is the intervention and not variance.

That is one bit, honestly obtained, and it is not a licence to spray values: without AGC's
convention, further forcing is guessing with extra steps. What it establishes is that the guest
*checks* this return, which is worth knowing before anyone implements it.

## A correction, recorded because the reasoning was the point

Mid-investigation I concluded the override had not applied, because
`0x53bbd82b51d172db was called 1 times and has no name` appeared in the forced run too. That was
wrong: a forced return is answered *at the stub*, so the call is still correctly reported as
unimplemented - a forced value is not an implementation. The run report had said so all along:

```text
! this run was under 0x53bbd82b51d172db answers 0x0 (1 answered) … this verdict measures a
  settings change
not recorded: this run was under a diagnostic
```

D224, D226 and D227 built exactly that line, and it was on screen while I was inferring the
opposite from a count. **The check that settles an intervention is the one the report already
prints**, not a proxy invented in the moment.
