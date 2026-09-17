# 593. Load-time initialisation is ruled out, and the wall it was asked about is gone

**2026-09-15** - step 4 of the gap analysis, answered twice over: the hypothesis is negative, and
its premise was stale

## The question

The gap analysis put this fourth: *"Rule load-time module initialisation in or out for the
PPSA02664/PPSA03416 wall"* - a run or two, against a lead it attributed to PROJECT_STATUS:

> What remains is not a call at all - established at load time, or computed by the guest from
> something it already has.

Module initialisation for dependent modules was the named suspect.

## The hypothesis is ruled out, by measurement

Both titles run, both report the same shape:

```
2 module(s) started: PS5Util.prx (1 initialiser(s), handle 0x40),
                     Il2CppUserAssemblies.prx (1 initialiser(s), handle 0x41);
loader placed with initialisers: Il2CppUserAssemblies(1 init), PS5Util(1 init), libc(1 init)
```

PPSA03416 is identical but for a fourth loader-placed module, `libSceAmpr`.

- Every module handed a handle **was** started. The `module(s) got a handle and were NOT started`
  line does not appear in either run - checked for, not merely unobserved.
- Every started module ran an initialiser. The `ran NO initialiser` callout does not fire.
- `run_initialisers` calls `DT_INIT` **and** walks every `DT_INIT_ARRAY` slot, skipping only the
  null and `-1` entries the ABI permits, counting each call that returned.

So "a guest calling into code whose constructors never ran" is not what is happening in either
title.

**What the measurement cannot say**, and it is worth being exact: the count is of initialisers
that were *called and returned*. A constructor that ran and gave up early - because something it
needed was refused - counts as one. The negative result is "nothing was skipped", not "everything
worked".

## The premise was stale, which matters more

PROJECT_STATUS's "three walls" section describes PPSA02664 / PPSA03416 at `image+0xafc959`,
writing to `0xfffe0`. **Both titles passed that wall.** They now reach `flipped` with 220 imports
and one frame each, and die in the AGC patch family - `sceAgcSetCxRegIndirectPatchAddRegisters`
carrying orbistoun's own `0xf7ff0001` placeholder into a `memcpy`, thirty-two times, because
`sceAgcDcbSetCxRegistersIndirect` is unimplemented and answers it (worklog 553, 2026-09-14).

So the sentence that sent this investigation to load time belongs to an investigation of a
different wall, and the lead it names was never about the wall these titles hit today.

That is the **third** stale document in two days, after `agc.rs` saying nothing was implemented
over sixteen handlers (worklog 589) and phase 6 posing a decision D695 then settled (worklog 592).
PROJECT_STATUS opens by saying an emulator status page that overstates itself is useless to its
own author six months later. This one understated, which is the same failure wearing the other
face - and it cost a step in somebody's plan.

Corrected: the bullet now leads with the current wall and the measurement above, and the `0xfffe0`
investigation is kept, explicitly, as the record of a wall that was. Those eliminations cost days
and are not repeatable for free.

## Surprise: a host address is not a reproducible outcome

Both titles previously recorded `outcome = "0x7fff13abdc8d"`. Today both report
`0x7ff9c071dc8d (was 0x7fff13abdc8d)` - the same fault, at a different number.

It is a **host** address. Windows bases system modules per boot, so it is stable within a boot
session - both runs today agree with each other - and moves across reboots. Two consequences,
neither previously written down:

- The recorded outcome for these two titles **cannot be reproduced after a reboot**, so a record
  that looks like a measurement of the guest is partly a measurement of where the host loader put
  something.
- `compare` reports the ending as changed whenever the machine has rebooted between runs, with
  nothing having changed at all - a false signal in the one measure of progress this project has.

Not fixed here: the fix is to record host-range faults as something stable - the module and offset
the address falls in, the way guest faults already render as `image+0x…` - and that is a change to
how faults are described rather than a note to leave in a worklog. Recorded as a defect worth its
own unit.

## Gate state

`cargo fmt --all --check` clean, `cargo test --workspace` 2,335 pass, worklogs unique, identity
scan clean. No code changed in this unit - two documents did.
