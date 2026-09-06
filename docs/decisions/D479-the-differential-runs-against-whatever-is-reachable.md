# D479 - The differential runs against whatever published implementation is reachable, and names it

**measured** - 2026-09-02 (user-directed plan, R7)

R7 was specified as a FreeBSD differential, on the reasoning that the target's C library is
FreeBSD-derived, so FreeBSD is the closest lawful analogue available. That reasoning stands.
**FreeBSD is not reachable on this machine, and glibc is.**

## What was actually in the way, which was not what was expected

RAM was assumed to be the constraint - the runner VM sits at 4G because Hyper-V would not
reserve 8G. It is not: the host has 31.9 GB with 14.2 free, which is ample for a 2 GB guest.

The real constraints are different and worth recording so nobody re-derives them:

- **Hyper-V is installed but not permitted.** `Get-VMHost` answers "You do not have the
  required permission to complete this task", so provisioning needs an elevated session.
- **multipass publishes Ubuntu images only.** It is installed and working, and has no FreeBSD.
- **No qemu, no VirtualBox, no vagrant.**
- A FreeBSD image is a multi-gigabyte third-party download, which is not something to do
  unasked.

**WSL2 works**, with glibc 2.39 and gcc 13.3, needing no admin and no download.

## The decision

The differential runs against **whichever published implementation of the same interface is
reachable**, and every record names which one. D478 already worded the tier that way -
"run against a published implementation of the same interface" - and made `needs_citation`
true precisely so a record has to say *which*. This is that clause being used rather than a
new rule.

The reference program prints its own library and version (`REF|library|glibc|2.39`), so a
committed run is self-describing and a FreeBSD run later sits beside it without ambiguity.

## What glibc agreement does and does not buy

**Does**: conformance to ISO C and POSIX, which is what the great majority of orbistoun's
mechanical surface implements. A divergence from glibc on a standard-specified function is
almost certainly an orbistoun bug, and four of the first twenty-four cases were exactly that.

**Does not**: any statement about the console. Where FreeBSD and glibc differ, glibc says
nothing about the target - and D468 is this project watching that gap open, when the
FreeBSD-*documented* ctype layout turned out not to be the platform's. This is why D478 kept
the tier probeable, and the reasoning applies with more force to a library that is not even
the target's ancestor.

**Cannot test at all**: anything BSD-specific. `strlcpy`, `strnstr` and the `_np` family are
not in glibc, so those cases have no reference here and are not silently skipped - the checker
lists a case it cannot rebuild rather than passing over it.

## The shape that keeps the two sides honest

The reference emits **the inputs it used** alongside its results, and the checker rebuilds each
call from those. A differential where each side carries its own copy of the cases is one that
silently stops comparing the same thing; with one list there is nothing to drift.

`check()` gained `also differential`, which rebuilds the reference program and diffs its output
against the committed run - the same property the generated-table check enforces, because a
recording that no longer matches its generator is a comparison against history. It warns rather
than failing where no reference library is reachable, since a step that cannot run has not
passed.

## Status

`measured`: the reference was compiled and run, the four divergences it found were real, and
three of them were fixed and confirmed by re-running. What remains assumed is the part D478
already covers - that any of this describes the console.
