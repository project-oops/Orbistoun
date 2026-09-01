# 2026-08-29 - A one-line disagreement about zero


`ftpsrv` printed two lines and everybody would read the second:

```text
main-prospero.c:49:malloc: error 0 (orbistoun has no message table)
Unable to change AuthID
```

The first was the cause. `malloc(0)` answered null, with a comment explaining that the
standard permits it and a conforming caller must not dereference either answer. Every clause
true about the standard; this emulator's job is the platform, FreeBSD answers a unique
pointer, and real callers write `if (!p) fail` (D383).

So a program that asked how many processes were running, was told none, and allocated nothing
for the list reported a memory failure and gave up on everything after it. It read as a
privilege problem.

**A permitted behaviour is not automatically the right one.** Where a standard allows two
answers the question is which one the thing being imitated does, because the callers are real
programs written against that thing.

Found only because a payload got far enough to name its own file and line. Nothing in this
project's reporting would have caught it: the call succeeded, returned a permitted value, and
was recorded as answered.

