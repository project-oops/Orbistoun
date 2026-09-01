# D193 - The titles carry their own names, and the generator could not spell one


**decided** · 2026-08-22

`0x48a758b2e731cfd7` blocked two Unity titles for weeks. It is **`sceKernelCreateSema`**,
and the string was lying in a *third* title's data the whole time.

### Why generation could never have found it

Naming is generate-and-test against a one-way hash, so the only question that matters is
whether the true name is in the candidate set. The vocabulary held `Semaphore`; the vendor
wrote `Sema`. `sceKernelCreateSemaphore` was generated and tested, 2.58 **billion**
candidates were tried in seventy-five seconds, and the real name was never among them.

**The missing thing was a spelling, not an idea.** No quantity of added guesswork closes
that reliably - the concept was already present and still produced nothing.

### Where the answer actually was

A title's diagnostics, assertions and symbol tables leave literal function names in its
data. Unity's `[SCE]` diagnostic is `scePthreadMutexattrInit(&mutexAttr) returned %s in
%s(%d)`, and that name is in the binary whether or not the error path ever runs - it was
only *seen* today because `printf` got implemented and the guest happened to fail (D186).

Scanning a module's bytes for identifier-shaped runs and hashing each one names **22
imports of one title in a single pass over a file already in memory**, against zero from
the full generated sweep. Audio, video, networking, pad, kernel, sysmodule.

Clean-room, and worth being exact about why: nothing is consulted. The bytes are the
guest's own, already read for its import table, and the hash confirms or rejects every
candidate exactly as it does a generated one. No database, no other project's source, no
recall.

### What a confirmed name is worth beyond itself

It is *not* re-derivable from this repository alone, because a title can never be in this
repository - so a name found this way would sit permanently unaccounted in the audit (D119).

Its **parts** fix that. `sceKernelCreateSema` contributes `Sema`, and every other
`sce…Sema…` import becomes reachable by generation, from the repository alone, without the
title. The search now reports the words each confirmed name contributes; one run against a
single title suggested fifty the vocabulary lacked, including `Equeue`, `Munmap`, `Ntoa`,
`Out2` and `Setaffinity`.

That is the loop closing on itself: observation names an import, the name teaches the
generator, and the generator reaches the next one unaided.

### The implementation, and what it does not know

Only the first argument is established - a stack address the guest expects filled - and
**writing it is the entire point**. Reporting success without writing would hand the guest
whatever its stack held and produce a failure with no signature anywhere, which is D171
exactly. The count arguments are inferred from the shape of semaphore interfaces generally,
recorded as an assumption, and clamped rather than trusted.

Both titles: 45 calls and an abort become **222 calls, 96% of them on real
implementations**, with no override and no configuration.

