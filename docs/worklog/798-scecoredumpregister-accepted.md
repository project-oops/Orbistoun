# 798. `sceCoredumpRegisterCoredumpHandler` is accepted honestly — the crash-handler registration succeeds rather than answering a placeholder; PPSA04263 standing rises 5→4 stubs, the second honest export of the hardening pivot

**2026-09-22** — worklog 797 landed the first export of the hardening pivot (`scePthreadGetaffinity`).
This is the second, and the cleanest of the accept-a-request family: a title's crash-handler
registration.

## The implementation

`sceCoredumpRegisterCoredumpHandler(handler, ...)` now answers `0`. A title registers a handler to be
told about its own faults; orbistoun catches guest faults itself and does not coredump, so the handler is
never invoked - but the registration **succeeds**, which is what the caller tests. Unimplemented it
answered the `0xf7ff0001` placeholder, a negative code that reads as a failed registration and can make a
title log or refuse; `0` is the honest answer that the request was taken, on the exact terms
`sceAppContentTemporaryDataMount2` (worklog 673) and `scePthreadSetaffinity` (D523) already set for
accepting a request orbistoun will not act on. `known_by = assumed`: the success return is the accept
convention, not a measurement, and nothing fabricates a handler id - the return is a status tested
against zero, not an object dereferenced. Removed from `SERVES_NOTHING`, wired into the service's
implementation list, knowledge entry upgraded from bare to behavioural, with a unit test watched failing
on the placeholder.

## Measured

PPSA04263 answers it from an implementation now: **4 functions on stubs, down from 5** (and 6 before this
session). Same `image+0x19676d7` wall (the un-run static constructor, worklog 794-796) - a crash handler
is nothing that wall is waiting on, and the run says `same` rather than dressing the standing bump as
progress. Two placeholder-lies gone this session for one title, honestly.

## The pivot's steady state

This is what a hardening tick looks like: pick a real export a title calls and answers with a placeholder,
answer it from published semantics or the established accept convention, gate, and record whether it moved
the wall (it did not, and that is said plainly). The remaining PPSA04263 stubs are `sceImeUpdate`,
`sceUserServiceGetGamePresets` and `sceKernelGetdirentries`; the last is a real `getdirentries(2)` analogue
with published semantics and is the next candidate worth the FS work, the other two needing measured
values rather than a convention.

## Gate state

Code changed: `orbistoun-systemservice` implements `sceCoredumpRegisterCoredumpHandler` (accepted with
`0`), wired into the service's implementation list and out of `SERVES_NOTHING`, with the `libSceCoredump`
knowledge entry upgraded (`known_by = assumed`) and a unit test. PPSA04263 standing rises 5→4 stubs, wall
unchanged. `./bin/orbistoun check` green, generated docs regenerated, worklog index regenerated, identity
scan clean. No commit.
