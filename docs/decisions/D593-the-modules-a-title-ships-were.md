# D593 - The modules a title ships were unreadable to every diagnostic

**Status:** measured
**Date:** 2026-09-08

## Two halves of one blind spot

A watchpoint on PPSA03416's command buffer named the instructions that build it -
`the title's own modules+0x2e66271` and its neighbours. Reading the bytes there is the obvious
next question, and it could not be asked:

```text
0x480002e66260+0x40 is in no span this run published as readable
```

**The modules a title ships had a name in the fault reporter and no entry in the readable spans.**
`describe_region(Region::TitleModules, …)` told the reporter where they are so a fault could be
named; nothing told the readers, whose list is the image and the main stack. So a watch on memory
the loader had itself placed answered exactly what an unmapped address answers.

And the second half: a watch on a region that *did* exist reported a diff and nothing else. When
nothing changed it said so and stopped, which is a complete answer to "what did the guest write
here" and no answer at all to "what is here" - and for anything the loader placed, the contents
are the question.

Both fixed. The span is published where it is described, and a watch that finds nothing changed
prints what it holds.

## Reading guest modules is ordinary, and always was

`docs/PROVENANCE.md` calls this `static` evidence: *"read out of guest material at rest, nothing
executed"*. It is the same category as a module's import table, which this project has read since
its first week, and as the string harvester that reads identifier-shaped bytes out of every
module to feed the naming loop.

What the boundary forbids is **other people's source** - a vendor SDK, another emulator's code.
A module in the user's own title directory is neither. Nothing here disassembles a vendor
library; what was unreadable was memory orbistoun had mapped itself.

## What it showed immediately

The four instructions that update the command buffer's header:

```text
45 01 67 08    add  dword [r15+0x8], r12d
41 89 5f 04    mov  dword [r15+0x4], ebx
41 09 07       or   dword [r15], eax
41 21 07       and  dword [r15], eax
```

Which **corrects the reading three entries have been built on**. `+0x08` is a count and is
incremented; `+0x04` is *assigned*, so `0x14` is a value rather than a running offset; `+0x00`
is a flags word, OR-ed and AND-ed with masks rather than a size. D587 said "which of the first
two words is the size, the command count and the current offset is not established", which was
the right amount of caution and still let the wrong shape into three later readings.

**And nothing in that sequence writes the storage buffer.** The header is bookkeeping; whatever
writes command bytes is elsewhere, or does not happen.

## What this does not establish

**What the fields mean.** Assigned-versus-incremented is a property of the instructions and says
nothing about intent. `0x14` is twenty and `0x10000` is a bit; neither has a name here.

**Nor that this is the only place the header is written.** Four instructions were read because a
watchpoint named them. A second site would look the same and has not been searched for.

**Nor that reading further is cheap.** This is a disassembly, done by hand, of sixty-four bytes.
It is admissible and it does not scale, which is the argument for the watchpoint naming the
address first rather than for reading the module through.
