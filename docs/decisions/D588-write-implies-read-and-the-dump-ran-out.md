# D588 - Write implies read, the dump ran out of room, and D580 was wrong

**Status:** measured
**Date:** 2026-09-08

## The claim that has to be withdrawn first

D580 recorded this:

> **In a run where that pointer read `0x740009200000`, the same run's watch on
> `0x740009200000` reported no published span** - down to an eight-byte window. The guest
> holds a pointer to a buffer orbistoun never mapped for it.

It caveated itself correctly - *the tool distinguishes unmapped from unpublished no better here
than anywhere else* - and the caveat was the load-bearing part. **The buffer is mapped.**
Asked from inside the call that receives it, where the two cannot be a run apart:

```text
orbistoun:   it points at 0x740009200000, inside a mapping of
             0x740009200000..0x740009300000 that nothing published for reading
```

One mebibyte, matching the header's own `0x100000` exactly. The guest was given precisely what
it is holding. `region_containing` is the authority - it consults the live map rather than the
list something published for diagnostics - and this crate could have been asked at any point.

**Two failures produced that entry**, and both are ones this project already names. The address
was typed in from a *previous* run while the arena moves between runs (D582), so the pairing was
never sound. And a message reported "no published span" in the words a reader takes for "not
mapped", which is the distinction the same session had just renamed the dump's message to make.
Renaming a message is not the same as believing it.

## Why it was not published: two causes, both real

**Write did not imply read.** `protection_from_guest` mapped guest `PROT_WRITE` to a host
protection with `read: false`, and this title maps its asynchronous-file destination buffer with
write alone. An x86-64 page table entry has a write bit and a no-execute bit and **no read bit**:
a writable page is readable, and there is no encoding that is not. POSIX anticipates it -
`mmap(2)` says an implementation may permit accesses other than those requested - and FreeBSD on
amd64 grants read with write for the same reason. Recording it as unreadable made orbistoun
stricter than the machine it presents.

**And the table was full.** `MOST_EXTRA_RANGES` was sixty-four, sized when the only thing
publishing there was one range per guest thread. D579 then had every guest *mapping* publish too,
and PPSA03416 reaches about a hundred and twenty - so everything past the sixty-fourth was
**silently dropped**, and an argument pointing into one reported exactly as a wild pointer does.

Raised to five hundred and twelve, which is eight kibibytes of statics. Fixed rather than
growable because it is read from the guest's own stack, where allocating is what D381 forbids.

## A dropped range now says so

```text
orbistoun: 60 readable range(s) could not be remembered - an argument pointing into one of
           them reports as unreadable, and that is this run's blind spot rather than a bad
           pointer
```

**The count is the point.** A capacity limit and a wrong pointer print identically, and the run
had no way to say which - so every unreadable argument in an overflowing run read as the guest's
mistake. That is principle 3 at the tool level for the third time this week, and the fix is the
same each time: say what was measured, including that the measurement did not happen.

## What it was all for

With both fixed, the buffer reads:

```text
orbistoun:   what it points at, 0x740009100000: 0x0 0x0 0x0 0x0 0x0 0x0 0x0 0x0
```

**Zero, while the header reports one command of twenty bytes.** That is a contradiction rather
than an answer, and it is recorded as one: `sceAmprAprCommandBufferReadFile` is imported by this
title and never called, so nothing here establishes how a command is meant to reach the buffer.

## What this does not establish

**That no other range is being dropped.** Five hundred and twelve is sixty more than this title
needs and nothing has measured a title that needs more. The count exists so the next one says so
rather than reporting its pointers as wrong.

**Nor that write-implies-read is right for every guest.** It is right for this architecture,
which is the only argument offered. A guest relying on a write-only mapping faulting on read
would be relying on something the hardware cannot do.
