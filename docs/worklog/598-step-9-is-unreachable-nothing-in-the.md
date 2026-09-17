# 598. Step 9 is unreachable: nothing in the corpus calls any of the four

**2026-09-15** - the gap analysis's last step, measured before it was started, and it is gated on
everything above it

## What was asked

Implement `libSceAvPlayer`, `libSceAjm`, `libSceSaveData_native` and `libSceMsgDialog.native` -
sized *weeks*, justified by: *"All four fail as **stalls, not faults**. A guest waiting on a player
that was refused, or on a dialog result that never arrives, runs a healthy-looking loop until the
limit."*

The reasoning is sound. The premise is not currently true.

## Not one call, anywhere in the corpus

Every unimplemented call in every title run today, which is the list these four would have to
appear on:

| Title | What it actually calls and nothing implements |
|---|---|
| PPSA02664 | `libSceAgc` - the patch family, 32 + 4 + 3 + 3 + 3 |
| PPSA03416 | the same, identically |
| PPSA04263 | `libkernel::scePthreadGetaffinity`, `libScePosix::pthread_setschedparam`, once each |
| PPSA25872 | `libSceAppContent::sceAppContentTemporaryDataMount2`, `libSceUserService::sceUserServiceGetAgeLevel` |
| PPSA99980 | `libSceNetCtl` ×55, `libkernel::statfs` ×55, `libSceNet`, `libSceKeyboard` |

**None of the four appears on any of them.** Their only appearance in a run is the loader's
load-time summary - *"22 unbound from libSceAvPlayer - no module this title ships exports under
that library name"* - which counts **declared imports**, not calls. A title naming a function in
its import table has not reached it.

## Why they are not reached, and why more time will not help

Each title stops somewhere earlier, and every one of those places is a different problem:

- PPSA02664 and PPSA03416 **fault**, in the AGC patch family (worklog 553), blocked on
  `REQ-20260914T1730Z-4386`.
- PPSA04263 **faults**, refused 4.51 GiB of direct memory it had already spent the pool on
  (worklog 574), blocked on `REQ-20260915T0030Z-5d1c`.
- PPSA25872 **faults** at `image+0x17554a3` (worklog 555).
- PPSA99980 is our own conformance probe, and its 55 repetitions are it probing rather than
  stalling. Worth saying, because the repetition *looks* like the stall this step describes and is
  not one.

A fault is not a stall, and a longer run does not reach past one. Worklog 597 measured the only
guest that keeps running: six times the clock, **zero** new imports.

**So the four libraries sit behind walls, and two of those walls are blocked on hardware
requests.** You cannot reach a title's first movie by implementing the movie player when the title
dies before the menu.

## What this says about the sequence

Step 9 is correctly last, and for a stronger reason than "it is big". It is the only step whose
value depends entirely on steps above it landing first: implementing all four today would produce
four subsystems nothing exercises, which is what principle 6 exists to prevent - *"writing the
audio shim before the address space works produces code that cannot be exercised, so it cannot be
trusted."*

The right trigger is mechanical rather than a judgement: **the first run in which one of the four
appears on a title's unimplemented-call list.** That list is printed by every run, so nobody has to
remember to check.

## What was refused

All of it. No player, no decoder, no save enumeration, no dialog. Four subsystems written against
no observed call would be four sets of guesses about structures nobody has measured - and the run
report cannot tell a correct one from a plausible one when nothing calls it.

## Surprise

**The diagnosis was right and pointed at the wrong libraries.** "Titles stall on something they
are waiting for" is a real failure shape and this project has exactly one instance of it - and it
is `libSceNetCtl::sceNetCtlInit` and `libkernel::statfs`, not any of the four. `statfs` is
especially worth noting: it is a POSIX function with a documented FreeBSD analogue, which puts it
in the strongest reference tier this project has (principle 1's first oracle), and it is called 55
times by a guest getting nothing back.

That is a cheaper and better-founded piece of work than any of step 9, and it came out of checking
step 9's premise rather than from the plan.
