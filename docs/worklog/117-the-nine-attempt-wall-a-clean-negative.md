# 2026-08-21 - The nine-attempt wall: a clean negative, after three false starts


`printf` made PPSA28061 legible. It loads ten texture files from `/app0` with correct sizes
and addresses - the filesystem layer is confirmed working rather than inferred - and then
reads address zero at `image+0x43c4`.

**The wall is not caused by any called stub's return value.** With all twenty-nine stubs the
title actually calls answering success, it faults in exactly the same place, with the same
address. That eliminates the whole "wrong return value" class for this wall, which is what
nine previous attempts had implicitly assumed.

I had claimed the opposite an hour earlier, on the strength of `default_return = "ok"` moving
the fault to `image+0x10b9e9`. That was over-read: the blanket sweep also drops the title
from 47 reached imports to 19, so the guest diverges early and dies somewhere else. **A
different path is not an escaped wall**, and the two are easy to confuse when only the fault
address is watched.

### Three failures in the search itself, all the same shape

The bisection script reported a confident answer three times before producing a usable one.

1. Relative paths with the session's working directory elsewhere - every `cargo run` failed,
   printed nothing, and the script read silence as "escaped the wall".
2. The candidate list written in text mode on Windows, so every entry carried a
   trailing carriage return. A CR inside a TOML basic string makes the whole config
   unparseable, the worker errors out, and again nothing is printed.
3. A guard added after (1) that reported silence correctly and could not stop the loop:
   `exit` inside `$(...)` leaves only the subshell. It printed its warning four times and
   was ignored each time.

Every one is the D187 shape - an experiment that fails in a way indistinguishable from a
result - and (3) is the sharpest: a guard that reports without halting is worse than none,
because the run looks instrumented and is therefore trusted more.

The rule that would have caught all three: **a measurement and the absence of a measurement
must never be the same value**, and whatever enforces that has to be able to stop the thing
it is watching.

### What is now known about the wall

- Not a missing import, and not a wrong return value.
- The guest reaches texture loading and beyond, printing its own progress.
- A null arrives from somewhere and is dereferenced at a low image address, in the
  neighbourhood of module-initialisation code.
- Remaining candidates: a side effect nobody performed - an out-parameter never written
  (D171's shape), or a module that was asked to load and did not.

`GuestStack::fill` (D185) has not been tried on this title and is the cheapest next probe.

