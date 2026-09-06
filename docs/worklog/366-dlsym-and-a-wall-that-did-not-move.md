# 2026-09-03 - (/loop) `dlsym` never knew the guest's own exports; the wall did not move

```
before   182 distinct, 415,331 calls, read of 0xa0 at image+0x1389269
after    181 distinct, 415,415 calls, read of 0xa0 at image+0x1389269
suites 124   tests 2006   clippy/fmt/identity clean
```

Third cron tick. Two findings, and the second is the honest half.

## `sceKernelDlsym` resolved against orbistoun's table and nothing else

`name_thunk` holds what **orbistoun implements**, so a guest asking for a symbol its own binary
exports was told it did not exist:

```text
orbistoun: the guest asked for the address of scriptingGetMem - which nothing here implements
```

The eboot's export table has exactly **one** entry and it is `scriptingGetMem`, at `+0xf25330`.
Orbistoun had that table all along - `raw_exports` reads it, `exports` prints it,
`PlacedTitleModules` indexes it for binding. `dlsym` was the one consumer that never asked.

Now: `answered 0x400000f25330`.

The lookup has to hash at the call - a guest asks by name, every export table is keyed by a
hash, and the loader cannot precompute a map for names it has not seen. So the loader registers
`(nid, address)` plus the suffix and the kernel hashes on arrival; `orbistoun-kernel` takes
`orbistoun-nid`, downhill on the spine. Thunk table first, guest exports second - purely
additive, so no name that resolved before changes its answer.

## And it did not move the wall

Three identical runs: same instruction, same fault. `sceKernelDlsym` is called **once** in the
whole run, so it was never going to be worth much to this title. It is worth having because it
is a real gap in a real mechanism - `dlsym` is the fourth-most-called import across the corpus,
141,717 calls over six modules - but for *this* wall it is not the answer, and saying otherwise
because the change was satisfying is the failure principle 3 is about.

## What the wall is, as far as it has been read

```text
4c 8b 35 a7 73 6a 00    mov r14, [rip+0x6a73a7]   <- r14 from a global at image+0x1A30610
49 8b 9e a0 00 00 00    mov rbx, [r14+0xa0]       <- FAULT
49 2b 9e 98 00 00 00    sub rbx, [r14+0x98]
```

A global holding a structure pointer, holding zero. The two fields are subtracted, which is the
shape of a used/allocated or end/start pair - a memory accounting read.

**Two hypotheses killed by measurement:**

- *One of the five remaining stubs.* Forcing all five to answer success reached three more
  imports and left the fault exactly where it was.
- *`scriptingGetMem` not resolving.* This change, and no.

## Breaks watched to fail

```text
dlsym does not consult guest exports  -> "its own symbol did not exist"      FAILED
the lookup answers the first entry    -> "worse than no lookup"              FAILED
```

## Two instrument notes, because both cost time

**`ORBISTOUN_RETURN` reaches unnamed functions by hash and named ones by name.** Given five
hashes for functions the symbol database has *named*, it matched nothing - it compares against a
label that carries the name. It said so, once per target, and I believed the negative for a
minute because my own grep filtered the warning out. Third time a filter's silence has been read
as the tree's.

**`ORBISTOUN_BSS_FILL` is too blunt here** - it killed the run at nine calls, because many
globals legitimately start at zero. It answers "does this run depend on uninitialised static
data", not "which global", which is what was wanted.

Decision: [D517](../decisions/D517-dlsym-never-knew-the-guests-own-exports.md).
