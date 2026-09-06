# D517 - `dlsym` never knew the guest's own exports, and the wall did not move anyway

**measured** - 2026-09-03 (three identical runs before and after)

Two findings, and the second one is the honest half.

## `sceKernelDlsym` resolved against orbistoun's table and nothing else

```rust
let address = orbistoun_thunk::name_thunk(&name);
```

`name_thunk` is the table of functions **orbistoun implements**. So a guest asking for a
symbol its *own* binary exports was told the symbol did not exist. PPSA02664 says so in one
line of its own run report:

```text
orbistoun: the guest asked for the address of scriptingGetMem - which nothing here implements
```

The eboot's export table has exactly **one** entry, and it is `scriptingGetMem`, at
`+0xf25330`. Orbistoun had that table the whole time - `raw_exports` reads it, `exports`
prints it, and `PlacedTitleModules` indexes it for binding. `dlsym` was the one consumer that
never asked.

Now:

```text
orbistoun: the guest asked for the address of scriptingGetMem - answered 0x400000f25330
```

## The lookup has to hash at the call, which is why the kernel gained a dependency

A guest asks `dlsym` for a **name**; every export table on this platform is keyed by a **hash**
of one. The loader cannot precompute the map, because it does not know which names a guest
will ask for and a hash cannot be reversed. So the loader registers `(nid, address)` and the
suffix, the kernel hashes at the call, and the two meet in the middle. `orbistoun-kernel` takes
`orbistoun-nid`, which is downhill on the spine.

Registration happens in two places because the two halves are known in two places: the placed
modules in `link_title_modules`, beside `note_module_initialisers` (D515); the executable in
`place_and_relocate`, which is where the service, the bytes and the placed image are all in
scope at once.

**Thunk table first, guest exports second.** Purely additive - every name that resolved before
resolves to the same address. Which should win when both answer is *not* settled here, because
nothing has been seen where both do; ordering it the other way would change what already-
working names answer, on no evidence.

## And it did not move the wall

```text
before   182 distinct, 415,331 calls, read of 0xa0 at image+0x1389269
after    181 distinct, 415,415 calls, read of 0xa0 at image+0x1389269
```

Three identical runs. The guest resolves its symbol, takes a slightly different path - one
fewer distinct import, eighty-four more calls - and faults at **the same instruction**. One
fewer import is a different path, not a worse position; the fault is the measure and it did not
change.

`sceKernelDlsym` is called exactly **once** in the whole run, so this was never going to be
worth many calls to this title. It is worth having because it was a real gap in a real
mechanism, and because `sceKernelDlsym` is the fourth-most-called import across the whole
corpus (141,717 calls over six modules) - but for *this* wall it is not the answer, and saying
otherwise because the change was satisfying is the failure principle 3 is about.

## What the wall actually is, as far as it has been read

```text
4c 8b 35 a7 73 6a 00    mov r14, [rip+0x6a73a7]     <- r14 from a global at image+0x1A30610
49 8b 9e a0 00 00 00    mov rbx, [r14+0xa0]         <- FAULT
49 2b 9e 98 00 00 00    sub rbx, [r14+0x98]
```

**A global holding a pointer to a structure, and it holds zero.** The two fields read are
`0xa0` and `0x98` and the code subtracts them, which is the shape of a used/allocated or
end/start pair - a memory accounting read.

Two hypotheses are already dead:

- **It is one of the remaining stubs.** Forcing all five to answer success -
  `scePthreadSetaffinity`, `scePthreadGetschedparam`, `sceKernelStat`, `sceKernelUuidCreate`,
  `sceKernelCreateEqueue` - reached three more imports and left the fault exactly where it was.
- **It is `scriptingGetMem` not resolving.** This change, and no.

## Two instrument notes, because both cost time

**`ORBISTOUN_RETURN` reaches unnamed functions by hash, and named ones by name.** Given five
hashes for five functions the symbol database *has* named, it matched nothing - the label it
compares against carries the name, not the hash. It said so, once per target, and the negative
was believed for a minute because the message was filtered out of the grep. The variable's own
summary is accurate; the mistake was mine, and it is the third time a filter's silence has been
read as the tree's.

**`ORBISTOUN_BSS_FILL` is too blunt for this question.** Filling `.bss` to find a global nobody
initialised killed the run at nine calls, because a great many globals legitimately start at
zero. It answers "does this run depend on uninitialised static data" and cannot answer "which
global", which is what was wanted here.
