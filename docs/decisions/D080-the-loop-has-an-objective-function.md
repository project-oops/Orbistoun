# D080 - The loop has an objective function

**decided** · 2026-08-19 · prompted by the user

The question was whether a fresh agent on a fresh machine could just be told
`./orbistoun.sh run <title>` and have the work be self-evident. Nearly - and finding the
gaps was the useful part.

**The loop had no measure of success.** It said what a guest wanted; it said nothing
about whether a change helped. Without a before-and-after, an iterative process is
repetition: you can change something, run again, and have no way to tell which direction
you moved. That is not a documentation problem, it is a missing number.

**The number is where the guest died.** A call trace now records the faulting
instruction, and a run compares it to the previous one:

```
progress
  FURTHER  image+0x13514 -> image+0x1a4c20
  imports  31 distinct (+2), 402 calls (+65)
```

An instruction pointer that moved forward means the guest **executed code it could not
reach before**, and nothing else in a run reports that as directly. Surviving where it
used to fault counts as the strongest forward signal there is. `BACK` is reported just as
plainly, because a change that makes things worse is worth knowing about immediately
rather than three changes later.

Deterministic in the way that matters: an unchanged tree gives the same fault address and
the same counts, so any movement is attributable to something that actually changed.

**Two supporting gaps closed at the same time.**

- `./orbistoun.sh doctor` says whether a machine can do the work and what is missing.
  Discovering that one failure at a time - a toolchain error, then a missing tool, then
  an empty directory - is how a two-minute setup takes an hour.
- `CLAUDE.md` now opens with three commands rather than a pointer at eighty decisions.
  The decision log is a reference, not an introduction, and telling someone to read it
  first was telling them to read the appendix before the book.

**What is still not automatic**, and should be said plainly: implementing a function is
work, not a command. The loop identifies the wall, measures whether you moved it, and
keeps the record - it does not write the implementation, and nothing here pretends it
will.

