# 763. No stub gates GTA's object construction - the null-vtable is a non-stub startup value

**2026-09-21** — closing the stub-suspect line of the PPSA04263 (Grand Theft Auto V) null-vtable
hunt with one decisive test, so the next session does not re-run it. The construction of the object at
`image+0x5b37e98` is **not gated by any stub**, which moves the whole investigation off imports and
onto a value orbistoun feeds startup.

## The decisive test

`ORBISTOUN_RETURN` forces a named import to answer a chosen value. Forcing **every** stub the run
hits to answer `OK` at once -

```text
ORBISTOUN_RETURN="sceUserServiceGetGamePresets:0,scePthreadGetaffinity:0,sceImeUpdate:0,
                  sceCoredumpRegisterCoredumpHandler:0,pthread_setschedparam:0"
```

- left the run at `verdict same, nothing moved`: the fault stayed at `image+0x19676d7`, `75` imports,
`30460` calls, unchanged. Individually, `GetGamePresets -> 0` and `Getaffinity -> 0` were each `same`
too. So no stub's *return* is what stops the object from being constructed.

## What is now ruled out, end to end

The construction of `image+0x5b37e98`'s vtable does **not** depend on:

- **The mutex** the report guessed (worklog 759 - it is a null-vtable virtual call, disassembled).
- **A skipped relocation or a truncated init array** (worklog 761 - `174172/174172` applied, complete).
- **Any stub's return** (this worklog - all five forced to `OK`, nothing moved).
- **`sceImeUpdate`'s return or callback** (worklog 762 - return ignored, no IME events to invoke it).
- **`fstat`** (worklog 761 - a real gap that moved the run `FURTHER`, but not this).
- **`Coredump`** (worklog 762 - `OK`, nothing moved).

The one hypothesis a return-force cannot reach is an **out-parameter** a getter leaves unwritten
(`GetGamePresets`/`Getaffinity` write nothing), but that would need the guest to *read* that buffer on
the construction path - and the construction is startup code, upstream of where those are called on
this run. The weight of the evidence is on a **non-stub value**: something orbistoun computes or hands
the executable's startup construction that differs from hardware, with no missing import involved.

## The remaining path, honestly

The only way left to *know* is the **ctor hunt**: find the instruction in the executable's startup
code that writes the vtable into `image+0x5b37e98`, and read what upstream value decides whether it
runs. That is disassembly of the executable's startup, not a single caller - a dedicated pass. Every
wall here is orbistoun's (D709); the title constructs this object on hardware, so the value orbistoun
gets wrong is findable.

This closes the tractable, cheap suspects. The null-vtable is real and cornered, but the next move on
it is heavier than reading one more stub - worth weighing against the more measurable walls elsewhere
(e.g. PPSA28061's `sceAgcGetRegisterDefaults2`, a named export obSCEne can probe).

## Gate state

No code changed - a characterisation recording the decisive stub-return test and the closed suspects,
so the ctor hunt starts from "not a stub" rather than re-deriving it. `./bin/orbistoun check` green,
worklog index regenerated, identity scan clean. No commit.
