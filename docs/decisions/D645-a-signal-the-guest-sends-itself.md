# D645 - A signal the guest sends itself

**Status:** measured
**Date:** 2026-09-09

## The wall moved, and this is where it stopped

D644 bound PPSA25872's sibling-module imports and took it from 20,000,000 calls at 2% standing to
**310,987 calls at 100% standing**. Everything it calls is now implemented, and it still does not
finish. The run report says "ran to the time limit", which is the report's honest reading of a
process that was still alive when the limit arrived - but it is the wrong picture:

| `ORBISTOUN_LIMIT` | calls |
|---|---|
| 20 (default) | 310,987 |
| 90 | 310,987 |

**Four and a half times the wall clock produced not one extra call.** The guest is not slow, it is
blocked, and the report's phrasing does not distinguish those (noted, not fixed here - it is a
separate change and this record is the measurement).

## What the last two calls were

The persisted trace ends on `libkernel_unity::sceKernelRaiseException`. Forcing dumps on it and on
the call before it gives both halves of one pair:

```
sceKernelInstallExceptionHandler(0x1e, 0x480001700210)
    -> title's own modules +0x1700210 = ff 05 02 be 00 00  83 ff 1e  75 16 …
sceKernelRaiseException(0x5e2d00000000, 0x1e)
    -> arg0 is orbistoun's own main pthread handle
```

`0x1e` is 30, and `SIGUSR1 = 30` was already harvested into
[`abi-constants.toml`](../../crates/orbistoun-hle/data/abi-constants.toml). The handler's own first
instructions confirm the argument order without any assumption: `inc dword [rip+0xbe02]` bumps a
counter, then `cmp edi, 0x1e` **compares the handler's own first argument against 30** - so the
signal number is passed in `edi`, and the guest wrote code expecting to see 30 there.

So: the title registers a handler for signal 30, raises signal 30 on its own main thread, and makes
no further call at any time limit. The shape is Unity/Mono's stop-the-world collector, which
suspends threads by signalling them - but that last sentence is `assumed`; everything above it is
measured, and the record keeps the two apart.

## Why this is recorded and not implemented in the same breath

Both functions were unknown to every knowledge file in the tree (`grep -c` answered 0 across all of
`crates/orbistoun-hle/data/knowledge/`), so the arity, the argument order and the signal number are
new facts and are now written down as `guest-observed`.

Implementing the pair is not a stub return. `sceKernelRaiseException` has to **run guest code on a
thread orbistoun does not own**, with the signal number in `edi`, and return only once the handler
has - and nothing measured says what it returns, on either side of that. Answering `Ok` because the
guest is waiting is precisely the plausible output principle 3 forbids: it would make the wall move
without anything having been determined, and the next wall would be built on a guess.

The honest next step is a measurement, not a stub, so the pair goes on the bus rather than into the
kernel crate.

## What follows

- `orbistoun-cli learn` entries for both functions, carrying the arity, the argument order derived
  from the handler's own `cmp edi, 0x1e`, and the handler prologue bytes.
- A bus request for the return values and the delivery order on hardware, with an `acceptance:`
  line naming both.
- Signal delivery stays unimplemented until that answers.
