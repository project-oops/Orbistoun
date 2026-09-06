# D474 - "Implemented" and "finished" are different states, and the report counts them separately

**measured** - 2026-09-02 (user-directed, from the question *"implemented vs complete - how can we catch these?"*)

The gap report counts whether an imported symbol **resolves to code**. It cannot tell a finished
function from one that answers the easy case and gives up, and both exist:

- `getopt` handles a process with **no arguments** and nothing else. An option that is actually
  present answers "no more options" rather than parsing it.
- `unsetenv` removes only what this layer holds. A variable the guest was *handed* arrived in the
  process image and cannot be taken out of it, so it is reported as removed and is still found.
- `scePthreadMutexInit` does not parse the attribute block, so a mutex the guest asked to be
  recursive is not one.

Every one of those was **already written down** - in a doc comment, honestly, at the point of
implementation. And every one counted as done, because a caveat a person has to read is a caveat
a report cannot subtract. That is the same shape as the five tools in principle 3's list: the
knowledge existed and the measurement did not consult it.

## The change

A `partial = "what is not done"` field on `FunctionKnowledge`, and a fourth row in
`worklist --static-gap`:

```
3 implemented but declared partial - present, and not finished:
  getopt
      An option that is actually present is not parsed: only the no-more-options answer ...
```

The field carries prose because the reader needs the *shape* of the gap, not a boolean - "does
not parse attributes" and "cannot remove what came from the image" are different problems with
different fixes, and a flag would flatten them into one.

## What it deliberately does not claim

**Empty does not mean complete.** It means no incompleteness has been *declared*, and the report
says exactly that when the list is empty rather than announcing that everything is finished. The
field is only as good as the honesty of whoever wrote the function, and it is not a conformance
check - it records what the author knew was missing, not what they got wrong without noticing.

For *that*, the oracle already exists and is not this: obSCEne speaks a command protocol and
`orbistoun-cli serve` answers the same one, so a driver can drive real hardware and this
emulator and diff the records live. Its `035-libc` section caught sixteen failures that way once.
The two are complements - this counts declared gaps, that one finds undeclared ones - and
neither substitutes for the other.

## A fix that fell out of the audit

Auditing the source for comments admitting incompleteness turned up `vfprintf`, which ignored its
stream argument and always wrote to the host's error stream. That was defensible when *both*
standard streams landed there anyway - and stopped being so in worklog 304, when `_Stdout` and
`_Stderr` became real wrapped descriptors and `fprintf` began routing by them. One of a pair
honouring a distinction the other ignores is worse than neither doing it, because the output
looks right until it does not. `vfprintf` now routes exactly as `fprintf` does.
