# 452. The generous window was zero

**2026-09-08** - directed, continuing 451

## An eighth casualty, and it looked nothing like the other seven

`015-sync/condvar-wakes-a-waiter` reports **"the waiter never reached the wait"** here and passes on
a console. That reads as a condition-variable or a thread-creation defect, and it is neither.

The check starts a waiter thread, sleeps **50 ms** - *"generous on purpose"*, its own comment says,
because too short "looks identical to a wakeup that does not work" - then reads a word the waiter
sets. The sleep is `sceKernelUsleep`, which under orbistoun is one of the weak definitions D626
found: no gadget, no payload args, `-1` at once. **The generous window is zero.**

Measured with the forced dump from D625, which is the first evening it could be:

```text
ORBISTOUN_DUMP=sceKernelUsleep,scePthreadMutexTrylock  ->  armed 3 of 1040 stub slot(s)
  ! libkernel::scePthreadMutexTrylock was asked about, and here is what it was passed
      arg0 = 0x600000800a40 -> stack+0x800a40 = 60 01 00 00 2d 5e 00 00 …
  (nothing at all for sceKernelUsleep)
```

The waiter **does** run and **does** reach its trylock; the sleep that was meant to give it time
never reaches this emulator. That alone produces the verdict. Whether the condition variable also
diverges cannot be told until the sleep works, and the record says so rather than guessing either
way (D626).

One declined call was producing findings in three unrelated sections.

## The corpus payload was 85 minutes stale, and it did not matter

`titles/obscene-payload/eboot.bin` was the 15:47 build; obSCEne's was 17:12 and 12 KiB larger -
the same shape of problem D622 recorded for the eboot and nobody had checked since. Refreshed, and
then **measured rather than assumed**: the new build reaches the same 223 imports and exits
cleanly, and `probe --against` gives byte-identical figures - 255 diverged, 115 pass-there, 17
distinct. So the staleness was harmless this time, which is a different statement from "the corpus
is fine" and is the one the evidence supports.

## The handoff route: one hypothesis tested and killed

D626 left the handoff argument as "worse, recorded not fixed". Two things this pass:

- **The gadget was never missing.** D626's own next step was to give the by-name stubs a syscall
  gadget at byte ten; `orbistoun_thunk::emit` has done that since D400 - bytes 2..15 of every thunk
  are `nop`, sliding into a jump to the gadget, a sled rather than a point because ten is one
  payload's number and not the platform's. Item withdrawn.
- **The kexport marker was not the cause either.** With no firmware skeleton the handoff block
  fills unknown fields with markers, and obSCEne branches on `pargs->kexport_table != NULL` in
  three places - so a marker there would send it walking a structure that is not one.
  `ORBISTOUN_HANDOFF_FIELDS=zero` tests exactly that, and changed nothing: same fault, same site,
  1729 calls against 1731. Hypothesis dead.

What is left is reproducible across both builds: the run resolves 569 names, bootstraps, prints
three of obSCEne's own progress lines, enters `obs_run_all`, and calls a null pointer at
`image+0x28a163` having emitted **zero** records. Going further needs that ELF's symbol table, and
nothing on this machine reads one - `exports` sees no dynamic exports and there is no `nm`.

## Surprises

- **Two hypotheses in a row were wrong, and both were cheap.** The gadget one cost a grep and the
  kexport one cost a single run. That is the ratio this project wants: D620 killed a week of AGC
  work for one flag, and the same shape held twice today.
- **`probe --against` was byte-identical across a build change**, which is a stronger statement
  about the differential's stability than anything designed for it.

## Next

- The handoff fault at `image+0x28a163`, which needs a symbol reader this machine does not have.
- The seven-plus casualties stay unmeasurable here until `REQ-20260908T1620Z-4c1e` lands or the
  payload gets a syscall route.
- The input group, blocked on `REQ-…-8bcf` and `REQ-…-3e71`, which are not mine to duplicate.
- `sceAgcCreateShader`, still the wall.
