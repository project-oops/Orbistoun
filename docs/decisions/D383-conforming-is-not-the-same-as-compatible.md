# D383 - Conforming is not the same as compatible


**decided** - 2026-08-29

`ftpsrv` printed `main-prospero.c:49:malloc:` and then `Unable to change AuthID`, and the
second line is what everybody would read. The first was the cause.

`malloc(0)` answered null. The comment said why, and the reasoning was sound: *"Zero is
allowed to return either null or a unique pointer. Null is simpler and a conforming caller
must not dereference it either way."*

Every clause of that is true about **the standard**. This emulator's job is the **platform** -
FreeBSD answers a unique pointer, and the near-universal caller idiom is `if (!p) fail`. So a
zero-sized request became an allocation failure, and a program that had asked how many
processes were running, been told none, and allocated nothing for the list, reported a memory
failure and gave up on everything after it.

It read as a privilege problem. It was a one-line disagreement about zero.

### The shape

**A permitted behaviour is not automatically the right one.** Where a standard allows two
answers, the question is not which is simpler or which a conforming caller could survive - it
is *which one the thing being imitated does*, because the callers are real programs written
against that thing rather than against the standard.

Worth holding next to `getcwd` answering the root and `setsockopt` applying to nothing: those
are stated divergences with reasons. This was a divergence nobody had noticed making, dressed
in a citation.

### And it was found by a payload getting far enough to complain

`ftpsrv` named its own file and line. Nothing in this project's own reporting would have found
it - the call succeeded, returned a value the standard permits, and was recorded as answered.
The measure of the day is that a guest could say so.

