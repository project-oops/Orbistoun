# D762 - An interrupting release posts the driver's end-of-pipe event

**Status:** decided
**Date:** 2026-10-08
**known_by:** measured (event 0, `INT_SEL` 2, context ids 0 and `0x101`); assumed beyond those

`sceAgcDriverAddEqEvent(queue, id, udata)` registers the driver's end-of-pipe event `id` against a
guest event queue. Every `RELEASE_MEM` that raises an interrupt posts one event to each queue
registered this way. That is a release whose `INT_SEL` (bits 26:24 of its selector dword, Mesa
`sid.h` `EOP_INT_SEL`) is not zero, and a flip's release is one too. The event is what the console
delivered (obSCEne REQ-20261008T1200Z-eo03, sweep 20261008-142306, `kevent-32b` rows):

- `ident` the registered id
- `filter` -14 (`EVFILT_AGC`)
- `flags` `0x20`
- `fflags` 0
- `data` the release's context id shifted left sixteen, over `0xff00`
- `udata` the registration's

A release with `INT_SEL` 0 raises no interrupt and posts nothing. The command processor no longer
reports one to the display either.

**Measured:** event 0 registered, `INT_SEL` 2, context ids 0 and `0x101`, each delivering exactly
the bytes above once the release retired.

**Assumed:** a registered id other than 0 is the event's `ident`; `INT_SEL` 1 and 3 post as 2 does;
a flip's release posts as well; a context id wider than sixteen bits keeps its upper bits above
`0xff00`. The one context measured with bit 27 set stalled the probe's queue (`-eo02`), so what
it posts is unknown.

**Why:** PPSA03416 and PPSA02664 register this event and block on the queue it is posted to.
Before, orbistoun answered the registration and posted nothing.
