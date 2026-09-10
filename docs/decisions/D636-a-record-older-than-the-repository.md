# D636 - A record older than the repository

**Status:** measured
**Date:** 2026-09-09

## The regression cannot be bisected, because the build is not here

D634 found PPSA28061 reaching 26 imports against a committed best of 47, and D635 built the
instrument that says so. The obvious next step was to bisect against the build that measured 47.

```text
compat/PPSA28061-app0.toml   measured_on = "2026-08-23"
git log --reverse            60030c0  2026-09-01  Initial commit
```

**The record is nine days older than the repository's first commit.** There is nothing to bisect
against; the build that reached 47 imports does not exist in this history and cannot be recovered
from it.

Checked across every record, because one instance is an anecdote: **PPSA28061 is the only one.**
Fourteen others carry dates before the initial commit, all `2026-08-31`, and every one of them
reports `imports = 0` - payloads that reached nothing, so there is nothing to reproduce.

## The ratchet is protecting exactly what it exists to prevent

`keep_status` refuses a backwards move, and its reason is written into the code:

> A run under a looser stub policy reaches further by construction, so ranking on the numbers
> alone would let one line of configuration permanently overwrite an honestly measured entry - and
> the database would then carry **a best-ever result that nothing can reproduce**.

That is the state it is now defending. The guard is right and the entry it guards is unreproducible
by a different route than the one the guard was built for - not a looser policy, but a build that
predates the repository.

**Not corrected here.** Deleting or overwriting a measurement is destructive and it is the
operator's data; the finding is worth more than the tidy-up, and D635's line now says the shortfall
on every run so nothing is hidden while it stands.

## What the title actually stops on

Worth having, because the regression is now a dead end and the *current* wall is not:

```text
! the guest called abort - it decided to stop rather than failing
    just before: libc::abort(0xbe9c0)                                   from 0x480000a1c7ad
    just before: libkernel::0x04df812afad225d7(0x600000800e20) -> 0x7fff0001 from 0x480000a1c760
    just before: libSceAgc::sceAgcCreateShader(0x4000004f6c68)  -> 0x7fff0001
    just before: libSceAgcDriver::sceAgcDriverRegisterDefaultOwner(0x0) -> 0x7fff0001
```

The guest calls an **unnamed libkernel function**, is handed the placeholder, and aborts
seventy-seven bytes later from the same function in its own module. That is a check-and-give-up,
and the name is the blocker: without it there is nothing to implement.

`sceAgcCreateShader` is two calls above, which makes this the third title in the corpus stopped in
the same neighbourhood.

## Seven unnamed, and four of them are answerable

| hash | can a vendor vocabulary name it? |
|---|---|
| `PS5Util::0xa96b2b178383025c`, `0xf316b8b2109d7727`, `0xf948d02a4f9f5ace` | **no** - the game's own module (D631) |
| `libkernel::0x04df812afad225d7` | yes, and it aborts PPSA28061 |
| `libSceAgc::0x53bbd82b51d172db` | yes |
| `libScePad::0xda809fad5f12799f` | yes |
| `unknown::0x384a0ae5cd37b0d4` | yes |

The generated search has already failed on all four, so the vocabulary is what is short. **obSCEne
walks the kernel export table** - `102-net/resolve:*:kexport` proves it reads one - but it looks up
a NID it computed from a name it already had, which cannot answer the reverse question.

An **enumeration** of that table's NIDs would settle up to four of these at once, one of them a
title's wall. Queued as the next bus request; two are open and the protocol says keep one or two.
