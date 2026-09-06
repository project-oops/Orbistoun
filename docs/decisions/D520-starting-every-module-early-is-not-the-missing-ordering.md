# D520 - Starting every placed module before entry is not the missing ordering

**measured** - 2026-09-03 (three interleaved pairs, identical each time)

D519 established that the wall is a **call ordering**: the guest reaches code needing its
memory manager before the line that creates it. D515 left one ordering question open in those
words:

> Whether the platform runs a placed module's initialisers at load time for modules bound as
> imports is unmeasured, and answering it by starting everything would be inventing behaviour.

`libc.prx` is exactly that case - placed, carrying one initialiser, and never started, because
nothing ever loads it by name. So the question is worth an experiment rather than an argument.

## The instrument

`ORBISTOUN_START_MODULES` runs **every** placed module's `DT_INIT` and `DT_INIT_ARRAY` after
protection and after the guest float environment is adopted, which is the last point before
the guest's own entry. Off by default, `Intervenes`, and it reports per module.

## The answer is no, and it is emphatic

```text
off   181 distinct, 415,335 calls, read of 0xa0 at image+0x1389269
on     15 distinct,     271 calls, read of 0x7fff0001
```

Three interleaved pairs, identical every time. **Starting everything does not move the wall
forward; it destroys the run**, and it fails in a way that says why: `read of 0x7fff0001` is
orbistoun's own placeholder error code being used as an address, so a constructor called
something nothing implements and wrote through the answer.

`Il2CppUserAssemblies` and `PS5Util` start and their constructors do real work - `_Znwm`,
`pthread_key_create`, `pthread_mutexattr_init`, `__cxa_atexit` - and then the third faults.

So the missing ordering, whatever it is, is **not** "initialise everything before main". A
whole class of explanation is eliminated rather than left arguable, which is what the
diagnostic was for.

## And the instrument failed the way it was built to catch

The first version printed what it had done **after** doing it. A constructor is guest code and
can fault, and one did - inside the third module - so the loop never returned and the line
never printed. The intervention read as though it had not run, which is exactly the ambiguity
D325 exists to remove, reproduced in a new diagnostic on its first use.

It now says what it is about to do before doing it, and what each module did is recorded as it
goes and reported from `persist`, which every ending reaches (D513). The evidence that it ran
survived only because that recording already existed.

## A handle nobody asked for is not reported as a handle

A module started this way has no handle, because no guest called `sceKernelLoadStartModule`
for it. Reporting one - even `0xffff_ffff_ffff_ffff` - puts a number in front of a reader that
a guest could be thought to hold. It reads `before entry` instead, and the test breaks on
exactly that wording:

```text
BREAK: invent a handle
  -> ARecordedModule (0 initialiser(s), handle 0xffffffffffffffff)   FAILED
```

## What this does not establish

**That the console does not initialise import-bound modules early.** It establishes that
orbistoun cannot do it *here, this way, at this point* without breaking a run that otherwise
gets 415,335 calls further. The console has a working libc before a title's constructors run;
orbistoun does not, and that difference is a better explanation of this result than anything
about the console's loader.

So the question D515 asked is still open. What is closed is the shortcut answer to it.
