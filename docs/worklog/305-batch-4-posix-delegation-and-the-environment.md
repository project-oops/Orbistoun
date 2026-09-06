# 2026-09-02 - (/loop) Bulk port batch 4: 20 POSIX delegations, and the arity check that refused two

```
documented   711 needed, 416 missing   ->   714 needed, 395 missing
```

## The POSIX question, answered with a number

The plan asked how many of the libScePosix gap were **delegation rather than new code**. Matching
the 281 missing names against everything orbistoun already implements, by normalising both sides
(lower-case, underscores dropped, `sce`/`Kernel`/`Pthread` stripped):

- **22 already exist** under a vendor name.
- **259 are genuinely absent.**

So the cheap win is real but small: the bulk of libScePosix is still work. Worth knowing rather
than assuming, which is why it was measured before anything was written.

## The arity check, which is the point of the exercise

The existing table carries a warning from D385: three names deliberately do *not* delegate,
because each vendor twin ends in a **name argument the POSIX one has no place for**, so
delegating read a register the caller never set and faulted the first guest to reach it.

That trap is live. Checking each of the 22 candidates' declared arity against its POSIX
signature - both written out independently, since the whole point is that they can differ:

| | |
|---|---|
| **20 agreed** and were delegated | |
| **2 refused** | `scePthreadBarrierInit` takes 4 to POSIX's 3; `scePthreadRwlockInit` takes 3 to POSIX's 2 |

Both refusals are the D385 shape exactly, and both are left unimplemented with the reason in
the table rather than delegated and hoped for. The 20 are the read-write locks, the barriers'
destroy/wait, the remaining attribute accessors, `mprotect` and `inet_pton`.

The crate's own tests do the guarding: `every_delegation_resolves_to_a_real_implementation`
compares the served count against the table's, so a row naming nothing fails; and
`every_served_name_is_declared` catches a served name with no declaration. Both pass.

## Also in

`setenv` and `unsetenv` (POSIX.1-2008), which needed the environment store hoisted out of
`environment_value` so both read and write reach **one table** - two would let `getenv` and
`setenv` disagree, which is the bug rather than the feature. Four tests, and the one that
matters is that `overwrite = 0` **leaves the existing value alone and still reports success**:
refusing and overwriting-anyway are both wrong and both look plausible. `unsetenv` documents
what it cannot do - a variable the guest was *handed* is in the process image and cannot be
taken out of it, so that case is stated rather than pretended.

`strtoumax` / `strtoimax` (C99 7.8.2.3-4), delegated to `strtoull`/`strtoll` since `uintmax_t`
is sixty-four bits here.

## A number not to misread

The `TitleOwn` row now says **0 needed, 0 missing**, and that does **not** mean the loader
problem is solved. `Il2CppUserAssemblies::setenv` was the only *named* import in that bucket,
and declaring `setenv` in libc moved its attribution: the registry resolves a NID to whichever
module declares it, so the survey now reports it under `libc`. The unnamed Il2Cpp hashes are
still in the no-name bucket, and `Il2CppUserAssemblies.prx` / `PS5Util.prx` still never load.
The row went quiet for a bookkeeping reason, not a real one - said here because a zero in a gap
report is exactly the kind of thing that gets read as done.

## State

clippy `--tests` clean - including the two `multiple_unsafe_ops_per_block` denials `setenv` hit
first time, split one operation per block as principle 4 requires. fmt clean, orbistoun-libc
(118) and orbistoun-posix tests pass, identity scan clean, nothing committed.

**Next**: stdio and math (the real `coshf`/`expf` family, not the runtime's `_F*` internals),
then the 259 genuinely-absent POSIX names, which is now the largest single block of documented
work left.
