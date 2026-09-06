# D498 - `strtol` converts the zero in `"0x"`, and a stale work item is not a work item

**measured** - 2026-09-03 (a new differential case, and a plan entry that had evaporated)

## `"0x"` with no digits after it

ISO C 7.22.1.4 defines the subject sequence as *the longest initial subsequence of the input
string that is of the expected form*. For `strtol("0x", &end, 16)` that subsequence is `"0"` -
so the call **converts**, answering zero, and `endptr` points at the `x`.

orbistoun consumed the `0x` prefix, found no hex digit, and reported no conversion at all:

```text
strtol/hex-prefix-no-digits: end_offset is 0x0, expected 0x1
```

The value was right by accident - a failed conversion also answers zero - and the `endptr` was
wrong, which is the entire thing `endptr` exists to distinguish. A caller separating "parsed a
zero" from "parsed nothing" got the wrong answer, and a caller walking a string with `endptr`
would have looped forever on `"0x"`.

Fixed by remembering where the prefix began: a prefix consumed with nothing usable behind it
falls back to the zero it started with, rather than to nothing at all.

**Found by adding sixteen cases, not by reading the code.** The differential is the only oracle
this project has that answers without a console, and it found four bugs on the day it was
built (worklog 320). This is one more, from sixteen cases that cost minutes to write.

## And a work item that had stopped existing

The plan carried `/dev/random` and `/dev/urandom` as the next thing to implement: the run
reports them as *"a work item, spelled by the thing that wanted it"*, and orbistoun's device
table holds only `/dev/klog`.

**The current run never asks for either.** That request belonged to the path the guest took
*before* D489 bound its imports into the title's own modules - the long placeholder path of
10,884 calls. It now dies at ~2,080, far short of whatever opened them.

Same shape as D494's 222 resolver calls: a work item derived from a run is a fact about that
run's path, and D489 changed the path. **The list needed re-deriving, not working through.**
Re-derived, the whole live work list is three items - the parked fault, and two pthread setters
that cannot be implemented without inventing an arity.

`/dev/random` is not withdrawn; D461 already settles how it would be served (deterministic by
default, a seeded stream, because a guest reads entropy to seed its own generator and cannot
tell). It is simply not reachable, so implementing it now would be building against a request
that cannot be reproduced, and no test could show it mattered.

## Which is why the blocked list moved to obSCEne

The two pthread setters are the honest kind of blocked: the arity is unestablished, nothing
declares them, and writing one would be inventing a signature - forbidden here (D008) and there
(obSCEne principle 2). Forcing both to succeed was measured to change nothing.

So they, and the two questions that need a console, are written down in
`obscene/docs/backlog/022-measurements-orbistoun-is-blocked-on.md`, on the side that owns the
hardware. A gap nobody has written down is one the next capture will not close.
