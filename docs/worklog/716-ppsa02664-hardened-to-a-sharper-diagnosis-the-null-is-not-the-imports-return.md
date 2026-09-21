# 716. PPSA02664, hardened to a sharper diagnosis: the null is not the import's return

**2026-09-19** — asked to pick a title and harden it, took PPSA02664 (Alex Kidd in Miracle World). It is one of the
two leading titles: it reaches **`flipped`** (renders a frame), runs 418,342 calls, and then faults
in Unity's `GfxDevicePS5SharedData::CreateWorkload()` - `read of 0xa8` through a null base register
(`r13`/`r14` = 0), inside `memcpy`/`VCRUNTIME140.dll+0x1dc8d` (worklog 672's wall, still standing).
This run did not move it past there, but it sharpened *what* the wall is and ruled a hypothesis out.

## The experiment: the null is not the import's return value

Worklog 672 named the likely producer of the null object: an unnamed `libSceAgc::0x7d86501b8094ef57`,
called in the same command-build loop. `-b7a1` asked for the sanctioned "answer it without
implementing" test - run under `ORBISTOUN_RETURN=0x7d86501b8094ef57:0x0` and record the outcome. Done:

- **Verdict `same`, nothing moved.** Same fault (`VCRUNTIME140.dll+0x1dc8d`, `read of 0xa8`), 418,342
  calls (−2, run-to-run jitter). `-b7a1` says a `same` result is a complete answer, and this one is.
- **The second observation (principle 3):** the guest did the same thing with `0x0` as with the
  `Unimplemented` placeholder. And the tell is in the fault address itself - it is **`null + 0xa8`**,
  not **`placeholder + 0xa8`** (`0xf7ff00a9`). If the guest dereferenced the import's *return*, the
  faulting address would carry whatever the return was; it carries zero. So the object the guest
  reads at `+0xa8` is **not** what the import returns.

That narrows the producer: the null object is a structure the import (or something in the loop)
should **write through one of its pointer arguments**, not hand back - so answering it needs the
call's *write* semantics, not a return value. Implementing it as a scalar-returning stub, however
the scalar is chosen, cannot fill the null. The allocator was considered and set aside: mspace/`new`
are served from the host heap (`lib.rs:848`, `arena::take` else `std::alloc::alloc`), which does not
spuriously return null at 418k calls, and no allocation sits in the calls just before the fault.

## Why this is where it stops, honestly

Naming `0x7d86501b8094ef57` is the bounded next step, and every route to it is currently closed:

- **Hardware is out.** obSCEne `-5e0b` (enumerate libSceAgc export NIDs) came back **not-possible** -
  retail PRX symbol tables are unmapped/protected on the console (ledger, worklog 674).
- **Files are out.** The name lives in `libSceAgc.sprx`'s export table, a system module; orbistoun
  ships no firmware (greenfield), so it is not in the title's own files.
- **The NID does not decode.** obSCEne could not link it and found no citable vendor name (`-b7a1`).

So the name now rests on clean-room SELFish mining, which is a separate effort, not an orbistoun
change. Inventing a workload-object layout to fill the null would be exactly the plausible output
principle 3 forbids at the last step before more of a frame. This is a wall to record precisely and
hand off, not to paper over.

## Disposition

- **`-b7a1` part 2 answered:** the override run is recorded here, verdict `same`, with the second
  observation. Part 1 (the `imports` binding column) is unrelated and blocked elsewhere
  (`orbistoun-loader/src` is not modifiable in this session).
- **PPSA02664's frontier is unchanged and blocked** on naming that one import; the sharper reading
  (return ruled out, so it is a write) is what a future session or a SELFish mining result acts on.
- No code changed. No commit.

## Gate state

No tree change beyond this worklog - a run, an override experiment, and a reading. `./bin/orbistoun
check` was green as of worklog 715; nothing here touches code. Identity scan clean.
