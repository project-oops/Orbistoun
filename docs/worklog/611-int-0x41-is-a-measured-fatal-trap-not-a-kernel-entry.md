# 611. `int 0x41` is measured: a fatal trap, not a kernel entry to implement

**2026-09-15** - the obSCEne measurement `int 0x41` was waiting on came back, and it refuted the class
the finding was built around

## The measurement

A new hardware sweep landed - `obscene/reports/hardware/20260915-174357-eboot.obs.log` - and it
carries the `int 0x41` probe REQ-...b3c2 asked for. I verified the rows in the log directly (the bus
has a live fabrication problem, so a resolution is only worth its `OBS|` rows; lines 2277-2282):

```
OBS|try|137-kernelcall/int41-probe|kernel|int 0x41
OBS|measure|137-kernelcall/int41-probe|sel-0x0|returned|0x0|bool
OBS|measure|137-kernelcall/int41-probe|sel-0x0|fault|0xa|signal
OBS|measure|137-kernelcall/int41-probe|sel-0x1|returned|0x0|bool
OBS|measure|137-kernelcall/int41-probe|sel-0x1|fault|0xa|signal
OBS|res|137-kernelcall/int41-probe|partial|0x0|int 0x41 raised signal; kernel did not return to user space|assumed
```

A bare `int 0x41` from userspace on retail, at selector `0x0` and `0x1`, **raises a signal (`0xa`)
and does not return**. This is the "fatal abort on hardware too" branch REQ-...b3c2 named as a valid
answer - and it redirects the whole investigation.

## What it refutes

The interrupt work (worklog 605, 608) was built on a hypothesis: `int 0x41` is a kernel service
orbistoun does not implement, so once obSCEne says what it reads and returns, a handler goes behind
`interrupt::install(0x41, ...)`. The measurement says there is **nothing to install**: `int 0x41` is
not a callable service on this platform, it is a fatal trap. A guest reaching it took a path that
faults on real hardware too - so a retail title that runs fine on hardware (PPSA04263 does, byte for
byte) never executes this `int 0x41` there. It reaches it under orbistoun because orbistoun fed it an
**upstream wrong value**, exactly like a `ud2` abort. The cause is what the guest was told just
before the trap, not the trap.

This is the same lesson as GTA's "missing asset" (worklog 603) and Earthion's mapper gate (worklog
610): the wall is a give-up reached via a wrong value orbistoun handed the guest, and the taxonomy has
to say so rather than send the reader after a handler that will never exist.

## What changed, in three places that agreed on the old story

`int 0x41` (only that vector - every other is still unmeasured and may be a real service) now
classifies as a trap whose cause is upstream, not a kernel entry awaiting a handler:

- **The ranked finding** (`orbistoun-report::diagnose::kernel_entry_finding`): `int 0x41` is a
  `Gap::Faulted`, "measured a fatal trap on retail ... not a kernel service", action "not orbistoun's
  to implement (obSCEne REQ-...b3c2) ... read the calls just before it - that answer is the gap, not
  the trap". Every other vector keeps `Gap::KernelEntryUnimplemented` and "characterise it, then add
  the handler".
- **The dispatch** (`orbistoun-turn`): because the gap is now `Faulted`, it routes to
  `SweepArguments` on the call leading in - the productive one-bit oracle - instead of `Person`
  ("wait for a device measurement", which is now done). The measurement turned a dead-end into a
  loop step.
- **The live crash print** (`orbistoun-worker::report::note_instruction_shape`): headed
  `GUEST TRAP (int 0x41, measured fatal)` with the upstream framing, so the `>>` print and the `!`
  finding no longer contradict each other - one said "characterise + add the handler" while the other
  will say "measured fatal, don't". Verified live on PPSA04263: both now say the same thing.

The interrupt mechanism (worklog 608) stays. It was correct infrastructure for a vector that measures
as a real returning service; `int 0x41` simply is not one. Its table stays empty for `0x41` - now
because the measurement says so, not because the measurement is pending.

## Made to fail

- `diagnose`: `int_0x41_is_a_measured_fatal_trap_and_an_unmeasured_vector_still_awaits_a_handler` -
  `int 0x41` is `Faulted` with "fatal"/"upstream" and no "characterise"; `int 0x42` is still
  `KernelEntryUnimplemented` with "characterise"; a `mov` is a plain fault. The `0x42` case is the
  guard against over-broadening: peel off only the vector that was measured.
- `worker`: `a_software_interrupt_names_its_vector_and_its_measured_category` - the crash print draws
  the same split, and a `ud2` and a `mov` stay where they were.
- `kernel`: `int_0x41_has_no_handler_because_it_is_measured_fatal` - the assertion is unchanged, its
  reason is not: measured fatal, so nothing to register (was "unmeasured, so not yet").

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,353 pass / 0 fail, worklogs unique, identity scan clean. Verified live on
PPSA04263: the crash print and the ranked finding both frame `int 0x41` as a measured-fatal trap
pointing upstream, and the finding routes to sweep the call before it.
