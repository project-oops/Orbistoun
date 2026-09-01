# D406 - A firmware skeleton crate, for guests that reach past the interface


**assumed** - 2026-08-30

`orbistoun-firmware` is a new crate. It reserves a large, mapped, zeroed region - a *skeleton*
of the console's firmware address space - so that a guest which reaches past the named interface
into raw memory lands in observable memory this project owns, rather than in an unmapped
sentinel.

### Why a crate and not part of orbistoun-kernel

`orbistoun-kernel` is the clean HLE of named calls - the interface an ordinary title uses. The
firmware image is the thing *underneath* that interface, which only the post-exploitation
payloads reach for. Keeping it separate keeps the two concerns from blurring: one answers "what
does `sceKernelDlsym` do", the other "what is at firmware+0x2885e00", and those are different
questions with different provenance.

### What it is and is not

Not a firmware. No dump, no keys, no vendor bytes - principle 1 stands. It is a region of the
project's own zeroed memory, at an address of the project's own choosing (`0xf0_0000_0000`),
offered as a base a guest's arithmetic can land in. A guest reading it gets zeroes, which is a
*stated placeholder*, and the fault reporter now names any address inside it as
`firmware+<offset>` so a payload's arithmetic is legible where a bare address hid it.

### What it bought, and the honest limit

It is stood up when a run presents a firmware (`machine.firmware != 0`), and a run that presents
none pays nothing. The fault reporter names its range. That is the accuracy-and-debuggability
foundation.

**It did not move `elfldr`.** The payload still stops at its own `ud2` (D402) *before* it reaches
any firmware arithmetic, so a mapped firmware region cannot help yet - the wall in front of it is
the noreturn-exit one, not the memory one. And a second obstacle surfaced trying to read the
payload's error status: the handoff structure this project hands a guest does not line up,
field-for-field, with the one the payloads expect, so a value placed in "field 5" here is not
read as field 5 there. That layout is defined by the loader the payloads were built for, and
this project neither has it nor can lift it.

So the crate is foundation, honestly labelled. The payloads sit behind a stack of blockers -
handoff-structure layout, the noreturn exit, and only then the firmware memory this crate
provides - and this is the last of the three, built first because it is the one wholly within
this project's control and the one that makes the other two debuggable.

