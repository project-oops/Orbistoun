# D157 - The mapping is parked behind a switch, not deleted and not shipped


**assumed** · 2026-08-20

`sceKernelMapNamedDirectMemory` is written, tested against hostile inputs, and **off by
default**. Enabling it takes PPSA28061 from 38 imports to 15 and moves the fault out of
the guest image into host code. Turning it off restores 38 exactly, so the cause is in
that function or in the path the guest takes because of it - but *where* is not
established.

Three options, and the reasoning for the third:

- **Ship it.** No. It regresses the only progress measure this project has, and a
  half-working mapping is worth less than none.
- **Delete it.** No. The name is confirmed, the implementation is reasoned and tested, and
  the next person to reach this wall would write the same code again.
- **A switch.** Yes - `[memory] map-direct-memory = true` in the config file. It is not
  dead code behind an `allow`, it is an experiment somebody can run, and the guest is the
  only oracle for why it regresses. Consulting that oracle now costs a config edit and a
  relaunch instead of a rebuild (principle 5).

**What is known, for whoever picks it up.** The last recorded call is the mapping call, so
the crash is inside it - and yet a print at the top of the function never appeared, nor did
one inside the dispatcher, with the strings confirmed present in the built binary. That
does not fit "my function faulted" and it is why this is parked rather than fixed: the next
step is a debugger on the worker process, not more reasoning.

Two hypotheses worth testing first, neither confirmed:

- The implementation runs on the *guest's* stack. Reserving memory takes far more stack
  than the stub path it replaced, and a guest close to its limit would fault somewhere with
  no relation to the cause.
- The reservation succeeds and the guest then uses the address in a way the mapping does
  not actually support - physical memory is not aliased, so two mappings of one physical
  range are two separate pieces of host memory.

