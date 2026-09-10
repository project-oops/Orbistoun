# 470. The guest signals itself

**2026-09-09** - directed, continuing 469

469 unblocked PPSA25872: 310,987 calls at **100% standing**, every import implemented. It still does
not finish, and the run report says "ran to the time limit". This pass asked what it was waiting
for.

## Blocked, not busy

```text
ORBISTOUN_LIMIT=20   310987 calls
ORBISTOUN_LIMIT=90   310987 calls
```

Four and a half times the wall clock, not one extra call. The work it does do is real - 168,970
`memcmp` and 59,207 `memcpy` - and then it stops dead.

The persisted trace ends on `libkernel_unity::sceKernelRaiseException`. Forced dumps on it and on
the call before give the pair:

```text
sceKernelInstallExceptionHandler(0x1e, 0x480001700210)
    -> +0x1700210 = ff 05 02 be 00 00   83 ff 1e   75 16 …
sceKernelRaiseException(0x5e2d00000000, 0x1e)
    -> arg0 is orbistoun's own main pthread handle
```

`0x1e` is 30 and `SIGUSR1 = 30` was already in `abi-constants.toml`. The handler's own second
instruction is `cmp edi, 0x1e` - **the guest's code compares its first argument against 30** - so
the argument order is read off the title's own bytes rather than assumed. It installs a handler for
signal 30, raises signal 30 on its own main thread, and never calls anything again.

## Recorded, deliberately not implemented

Neither function existed in any knowledge file (`grep -c` answered 0 across all of
`crates/orbistoun-hle/data/knowledge/`). Both are now `guest-observed` entries in
`libkernel_unity.toml` carrying the arity, the argument order, the signal number and the handler
prologue.

Implementing them is not a stub return: `sceKernelRaiseException` has to run guest code on a thread
this project does not own, with the signal number in `edi`, and return once the handler has.
Nothing measured says what either call returns. Answering `Ok` would move the wall without anything
having been determined - the failure principle 3 names, and the one an unattended run drifts toward
because the wall moving *feels* like progress (D645).

## Surprises

- **100% standing is not a finish line.** Every previous wall was a missing implementation, so
  "everything it calls is implemented" read like success. Here it means the opposite: the guest is
  satisfied with every answer it got and is waiting on something it was never given.
- **The report cannot tell blocked from slow.** "Ran to the time limit" is true and useless; the
  two-limit comparison is what separated them, and it is a manual step nothing prompts for.

## Next

- The bus request for the pair's return values and delivery order.
- The AGC contract - still three titles' wall (D641).
- The libc data-object request, still open, still PPSA21564's wall.
