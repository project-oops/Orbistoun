# 2026-09-03 - (/loop) Starting every module early is not the missing ordering

```
off   181 distinct, 415,415 calls, read of 0xa0 at image+0x1389269
on     15 distinct,     271 calls, read of 0x7fff0001
suites 124   tests 2006   clippy/fmt/identity clean
```

Sixth cron tick. D519 said the wall is a **call ordering**. D515 had left one ordering question
open in those words: *whether the platform runs a placed module's initialisers at load time for
modules bound as imports is unmeasured, and answering it by starting everything would be
inventing behaviour.* `libc.prx` is exactly that case - placed, one initialiser, never started.

So: an experiment rather than an argument.

## The instrument

`ORBISTOUN_START_MODULES` runs **every** placed module's `DT_INIT`/`DT_INIT_ARRAY` after
protection and after the guest float environment is adopted - the last point before the guest's
own entry. Off by default, `Intervenes`, reports per module.

## The answer is no, emphatically

Three interleaved pairs, identical every time. Starting everything does not move the wall
forward, **it destroys the run** - and it fails in a way that says why: `read of 0x7fff0001` is
orbistoun's own placeholder used as an address, so a constructor called something nothing
implements and wrote through the answer.

`Il2CppUserAssemblies` and `PS5Util` start and do real work (`_Znwm`, `pthread_key_create`,
`pthread_mutexattr_init`, `__cxa_atexit`); the third faults. A whole class of explanation
eliminated rather than left arguable.

## The instrument failed the way it exists to catch

The first version reported **after** doing the work. A constructor is guest code and can fault,
and one did - inside the third module - so the loop never returned and the line never printed.
**The intervention read as though it had not run**: exactly the ambiguity D325 removes,
reproduced in a new diagnostic on its first use.

It now says what it is about to do first. The per-module record survives a fault because it goes
through `persist`, which every ending reaches (D513) - that recording is the only reason the
evidence existed at all.

## A handle nobody asked for is not a handle

A module started this way has none, because no guest called `sceKernelLoadStartModule`. It reads
`before entry`, and the break is on that wording:

```text
BREAK: invent a handle -> ARecordedModule (0 initialiser(s), handle 0xffffffffffffffff)  FAILED
```

## What this does not establish

That the console does not initialise import-bound modules early. Only that **orbistoun cannot,
here, this way**, without breaking a run that otherwise gets 415,415 calls further. The console
has a working libc before a title's constructors run; orbistoun does not, and that is a better
explanation of this result than anything about the console's loader.

D515's question is still open. The shortcut answer to it is closed.

Decision: [D520](../decisions/D520-starting-every-module-early-is-not-the-missing-ordering.md).
