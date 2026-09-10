# D648 - Half a signal contract is worth implementing

**Status:** measured
**Date:** 2026-09-09

## What came back

`REQ-20260909T1215Z-6d31` asked obSCEne for the return contract and the *ordering* of
`sceKernelInstallExceptionHandler` and `sceKernelRaiseException`. Sweep 20260909-140114 answered
with both symbols resolved as `libkernel` exports and six return values:

| call | return |
|---|---|
| `install(30, handler)` | `0x0` |
| `install(30, other)` | `0x8002_0023` |
| `install(30, NULL)` while 30 is installed | `0x8002_0023` |
| `remove(30)` | `0x0` |
| `raise(1, 30)` with a handler installed | `0x8002_0003` |
| `raise(1, 30)` with the handler removed | `0x8002_0003` |
| inverted `raise(30, thread)` | `0x8002_0016` |

The inverted call confirms the argument order independently of PPSA25872's own `cmp edi, 0x1e`.

## The ordering question was not answered, because the probe never reached it

Every raise passed the literal thread handle `1`, so every raise failed on the handle and **no
handler ever ran**. The one bit the request was built around - is the flag set when `raise`
returns - is not in the sweep. That is not a complaint: it is the difference between the branches
that can be implemented and the one that cannot, and it is what this record is about.

## Implemented: install and remove. Refused: the delivering raise

`install` records the handler and answers `0x0`, or errno 35 for a duplicate. `remove` answers
`0x0` for a signal that has one. `raise` answers `ESRCH` for a handle this process never issued -
and **the loud placeholder for everything else**, including the case PPSA25872 actually makes.

Answering `0` there would move the wall today. It would also decide, silently and without
evidence, that the call is asynchronous - because a synchronous call returning before its handler
ran is not a contract any platform has. The next wall would then be built on that guess, and the
guess would be invisible. This is the same trade D645 refused before the measurement existed, and
the measurement did not change the part it refused.

**The `EINVAL` for signal 31 is not modelled either.** `SIGUSR2` is 31 in this platform's own
harvested headers, so "31 is not a valid signal" cannot be what it measured; and the same run
answers `ESRCH` for signal 30 with *no handler*, which the "no handler" reading would make
`EINVAL` too. Two readings, one observation, no way to choose. It is left out and asked again.

## The gloss was wrong and the number was right

The sweep glosses `0x8002_0023` as "35 = `EEXIST`". 35 is right; `EEXIST` is **17** in the
FreeBSD-derived numbering this project harvested from the platform's own headers, and 35 is
`EAGAIN`. `orbistoun_core::errno::AGAIN` is therefore recorded as 35 with the measurement as its
provenance and the gloss explicitly rejected in its own doc comment.

A wrong constant *name* is worse than a wrong value, because it is reused. Every later
implementation reaching for `EXISTS` would have got 35 and been right by accident until the day
something needed the real 17. Consuming a sibling project's measurement means consuming the
numbers and re-deriving the names.

## What it did to the run, and what "BACK" meant

PPSA25872: stub calls **8 → 6**, both calls now answered as measured. The guest still goes quiet
0.2s into a 20-second run, because the branch it needs is the refused one.

The report said `BACK  reaching less of the interface than it did` - 140 distinct against 141.
That was worth chasing rather than explaining away, so both builds were run and their label sets
compared: **identical, 114 labels each**. Nothing the guest reaches was lost. `distinct` counts
symbol *indices* with a nonzero count, and two indices can carry one label, so implementing a
function can move the count without moving the guest. The verdict is not wrong about what it
measures; it is measuring something narrower than the sentence it prints.
