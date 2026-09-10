# 460. A record older than the repository

**2026-09-09** - directed, continuing 459

Bus idle a ninth pass. The queued bisect ran into a wall of its own, which turned out to be the
finding.

## There is nothing to bisect against

```text
compat/PPSA28061-app0.toml   measured_on = "2026-08-23"
git log --reverse            60030c0  2026-09-01  Initial commit
```

The record that says 47 imports is **nine days older than the repository's first commit**. The
build that measured it does not exist in this history.

Checked across every record rather than stopping at one: PPSA28061 is the only case. Fourteen
others predate the initial commit too, all `2026-08-31`, and every one reports `imports = 0` -
payloads that reached nothing, with nothing to reproduce (D636).

`keep_status`'s own reason for refusing a backwards move is *"the database would then carry a
best-ever result that nothing can reproduce"*. That is the state it is now defending, arrived at by
a route the guard was not built for - not a looser policy, but a build older than the repository.
Left uncorrected: overwriting a measurement is destructive and the operator's call, and D635's line
now reports the shortfall on every run, so nothing is hidden while it stands.

## What the title stops on now, which is not a dead end

```text
! the guest called abort - it decided to stop rather than failing
    just before: libc::abort(0xbe9c0)                                   from 0x480000a1c7ad
    just before: libkernel::0x04df812afad225d7(0x600000800e20) -> 0x7fff0001 from 0x480000a1c760
    just before: libSceAgc::sceAgcCreateShader(0x4000004f6c68)  -> 0x7fff0001
    just before: libSceAgcDriver::sceAgcDriverRegisterDefaultOwner(0x0) -> 0x7fff0001
```

An **unnamed libkernel function** answers the placeholder and the guest aborts seventy-seven bytes
later, from the same function in its own module. A check-and-give-up, and the name is the blocker.
`sceAgcCreateShader` sits two calls above it, making this the third title in the corpus stopped in
that neighbourhood.

## Four of the seven unnamed hashes are answerable

Three are `PS5Util`, the game's own module, which no vendor vocabulary can name (D631). The other
four - one in libkernel that aborts this title, one in libSceAgc, one in libScePad, one whose
library is unknown - are vendor symbols the search has already failed on, so the vocabulary is
short rather than the method wrong.

**obSCEne walks the kernel export table** - the `102-net/resolve:*:kexport` records prove it reads
one - but it looks up a NID computed from a name it already has, which cannot answer the reverse
question. An enumeration of that table's NIDs would settle up to four at once, one of them a wall.
Queued as the next request; two are open and the protocol says one or two.

## Surprises

- **Six candidate names tested by hand, none matched.** Then I stopped, because guessing one at a
  time is what the generated search exists to replace, and it has already been run.
- **The abort is 0x4d bytes after the call it checked**, in the title's own module. The guest reads
  the return, branches, and quits - which is the most legible kind of wall there is, and it is
  waiting on a name rather than an implementation.

## Next

- File the kexport-enumeration request when a slot frees.
- The three relocation walls (D633), waiting on scope.
- `ORBISTOUN_DLSYM_STUBS` as a default (D632), waiting on a decision.
