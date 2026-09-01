# D127 - Two invented rules in one afternoon, and the tests caught both

**decided** · 2026-08-20

D126 recorded one rule I invented on top of a source rather than reading it from the
source. There were two, and the second was found only because a test refused to pass.

**The harvester looked for files named `Symbol.map`.** FreeBSD's threading library
declares its exports in **`pthread.map`**, so a harvest that announced 2,497 names
successfully had lost every `pthread_*` symbol - including `pthread_create` and
`pthread_mutex_lock`, both of which real titles import.

Now any `.map` file: the format is what makes a file relevant, not what somebody named it.
2,497 became **2,637**, and matches against a real import table went 123 to 125.

### What actually caught it

A test asserting that four named functions survive the harvest. It was written as a shape
check and turned out to be a coverage check - `pthread_create` failing is what exposed the
missing library.

It is now explicitly **one name per library**, because that is the property worth holding:
a harvest that silently drops an entire library still reports success, and the total looks
plausible either way. 2,497 is not obviously wrong. Only a missing name is.

### And one absence that is correct

`clock_gettime` is in no version script at all - FreeBSD generates syscall stubs from
`syscalls.master` at build time. The test now asserts it is **absent**, with a note saying
why, so that if it ever appears somebody is told the reasoning is stale rather than left
to wonder.

That is the honest shape for a known gap: pin it, explain it, and make its disappearance
noisy.

### The uncomfortable part

I reported these crates clean while two tests were failing. My verification loop counted
lint warnings and summed test *totals*, and never looked at failures - so a red suite
summed to a number and passed inspection.

A check that cannot fail is not a check. Both of the afternoon's real bugs - the reserved
name filter and the filename - were found by tests I had already written and then not
read the results of.

