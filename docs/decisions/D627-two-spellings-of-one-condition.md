# D627 - Two spellings of one condition gave the encoding

**Status:** measured
**Date:** 2026-09-08

## Three codes, and no way to read any of them

D617 recorded two libSceNet error codes because the value is the point: `sceNetRecv` answers
`0x8041_0123` on a connected socket with nothing to read and `0x8041_0139` on a listening one, and
whoever writes the function would otherwise invent a constant. A third arrived later -
`sceNetBind` refusing with `0x8041_0130`.

Three numbers, and nothing said what any digit of them meant. The kernel's codes are
`0x8002_0000 | errno` and audio's are `0x8026_0000 | errno`, so by analogy these would be
`0x8041_0000 | errno` - which would make the errnos `0x123`, `0x139` and `0x130`. Those are 291,
313 and 304, and no BSD errno list has them.

## The sweep exercised both layers, and recorded them side by side

The `20260908-170447` payload leg ran the same two conditions through the **POSIX** calls as well,
and read `__error()` after each:

| condition | `sceNet*` | `errno` | in hex |
|---|---|---|---|
| recv, connected socket, nothing to read | `0x8041_0123` | 35 `EAGAIN` | `0x23` |
| recv, listening socket | `0x8041_0139` | 57 `ENOTCONN` | `0x39` |

**`base = 0x8041_0100`.** Both fit, and the low byte is the ordinary BSD number after all - the
`0x01` that looked like part of the errno is part of the base.

`sceNetBind`'s `0x8041_0130` is consistent: `0x30` is 48, `EADDRINUSE`, and the POSIX `bind` in
that same check had already bound the address. Corroborating, not confirming - nobody read that
one's errno.

## Why the pairing is the whole of it

Either code alone says nothing about which byte carries the errno. `0x0123` splits as `01|23` or
`0|123` and there is no way to choose. It is having **the same condition in two spellings from one
run** that fixes the split, and that only exists because the probe exercises both layers and
records the raw values of each rather than a reading of them.

Nothing about this needed a new measurement. It needed two existing ones put next to each other,
which is what a differential is for and what a list of three unexplained constants is not.

## Recorded as a constant with a negative test

`orbistoun_net::NET_ERROR_BASE`, with a test reconstructing all three codes from their errnos, and
a second test asserting that `0x8041_0000 | EAGAIN` is **not** what the console answered. The
negative one is the point: every other subsystem's base ends in four zero digits, so this is the
value somebody will one day "correct", and the wrong version is wrong by exactly `0x100` on every
code - close enough to look right in a log and never equal to what a guest compares against.

Stated at the strength the evidence supports. Two points fit a line; what would falsify it is a
libSceNet code whose low byte is not a BSD errno for the condition that produced it, or one above
`0x8041_01ff`. Neither has been seen, and neither has been looked for.
