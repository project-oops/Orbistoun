# D709 - user-provided retail titles are always working; never blame the title

**Status:** accepted
**Date:** 2026-09-21

## Context

D708 said a wall is orbistoun's *until hardware proves it the title's*, and left an escape hatch:
"a title *can* be a broken dump or a dev build." That hatch was still wide enough to walk through,
and a session did.

PPSA04263's `int 0x41` was the bane of the project for weeks. It drew an enormous amount of real,
good engineering - the fault reporter that reads heap objects, `ORBISTOUN_PEEK`, the single-stepper,
the call-before-the-fault taxonomy, a POSIX filesystem layer, and a cross-platform CI pass - all
built to explain *why the guest reached a fatal trap*. And then worklog 620 concluded the trap was
"the title's own il2cpp assertion" and the file opens beneath it were "faithful", and **stopped** -
the exact cop-out D708 named, reached through D708's own hatch ("faithful ENOENT on a devkit path").

It was wrong, and this session measured why. The guest opens and stats `/host//ap/rpf.cache`,
orbistoun served no `/host` mount, and **the file existed the whole time** - at the title's own root,
guest path `/app0/rpf.cache`, in the runtime data directory (not the repo's `titles/` stub, which is
where a search had looked). Staging the real cache where the guest reads it moved the wall from
`image+0x196b91a` to `image+0x19676d7`, `+192` calls and `+3` imports - a genuinely new wall (a
`scePthreadMutexUnlock` return used as a pointer), also orbistoun's. So the entire `int 0x41` saga
was a downstream symptom of one unchecked upstream fact: a file's existence and where the run reads
it from.

The operator's directive, recorded verbatim in intent: **all retail titles provided to orbistoun are
to be assumed working on hardware; the operator does not hand over broken titles; "broken", "debug",
"devkit", "faithful failure" are never conclusions.**

## Decision

**For any title the operator provides, the title is working. Every wall is orbistoun's, with no
hardware-proof step required to establish that.** This amends D708:

1. **D708's escape hatch is closed for operator-provided titles.** "A title can be a broken dump or a
   dev build" does not apply to them. There is no observation that makes the title the answer -
   the operator warrants it runs on a console, and that warrant stands in for the hardware
   attestation D708 asked for. The default attribution is not merely "orbistoun until proven
   otherwise"; it is "orbistoun", and it does not get disproven by anything about the title.

2. **"Debug build", "devkit path", "faithful ENOENT/abort", "the title's own assertion" are banned as
   wall-closers.** They may appear only as a *restatement of the symptom*, never as a reason to stop.
   A `/host` "devkit" path is orbistoun's mount to serve, not the title's mistake to excuse.

3. **Check the trivial upstream fact before building downstream machinery.** Before instrumenting a
   fault, telemetering a trap, or reasoning about an assertion several frames up, verify the cheap
   thing the fault sits on top of: *does the file exist, and where does the run actually read it
   from?* The run loads a title from the resolved **data directory**, not the repo `titles/` stub -
   `orbistoun paths` prints the resolved locations. A failed `open` is orbistoun misrouting until the
   file is shown absent from the place the run reads. Heavy diagnosis of a downstream crash, when the
   cause is an unchecked file route, is the specific waste this rule exists to prevent.

## Consequences

`THE_LOOP.md` gains this rule beside the loop it governs, so it is read every tick. The debugging
infrastructure the `int 0x41` chase produced stays - it is valuable, and it is what finally read the
poisoned object and confirmed the cause - but the process order inverts: the cheap upstream check
comes first, and the machinery is for what survives it. D708 stands for titles whose provenance is
unknown; for operator-provided titles, D709 governs and the title is never the answer.
