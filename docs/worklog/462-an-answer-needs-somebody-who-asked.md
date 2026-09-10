# 462. An answer needs somebody who asked

**2026-09-09** - directed, continuing 461

Bus idle an eleventh pass. 461 concluded the local work was done, so this pass went looking for a
new issue instead - the `obscene` eboot, which runs to the time limit at 100% standing and whose
source is in the next repository.

It found a defect I introduced this morning.

## Two working functions at the head of the findings

An ordinary run - no diagnostics, nothing asked for - opened with:

```text
what to do about it
  ! libc::acos was asked about, and here is what it was passed
  ! libc::asin was asked about, and here is what it was passed
```

Nobody asked about either, and the real finding - `libScePad::scePadReadState`, 180 calls, nothing
implementing it - was pushed down the page by two functions that work.

`Gap::Captured` (D625) filtered on *having a dump*. Dumps are taken for every unimplemented import
too, and those are normally claimed by the `unimplemented` finding - so the leak is the category
that has a dump and **no** unimplemented finding, which nobody had thought about:

**The dump condition tests the integer handler.** A function answering in `xmm0` is bound to the
float table (D268), so its integer handler is `None`, so it is dumped by default - and it *is*
implemented, so nothing else claims it. Two tables disjoint by construction, one condition aware of
only one of them.

`arm_dumps` already knew which imports a run named; it printed the count and discarded the labels.
They are recorded on the trace now and `captured` fires only for them (D637). Ordinary run: none.
`ORBISTOUN_DUMP=sceKernelWrite`: exactly one. `scePadReadState` back at the top.

Tested both ways, because a version emitting nothing at all would pass the first assertion - and
emitting nothing is exactly what this finding did for its whole first day.

## Surprises

- **The regression was mine, from this morning**, in the same session as eight records about
  instruments that could only say one of the two things they appeared to say. Writing those did not
  stop me shipping one.
- **It was found by running a guest for an unrelated reason.** I opened the `obscene` eboot to look
  at why it runs to the time limit, and never got there - the top of its report was wrong.

## What that says about the loop

The report is the instrument that checks the other instruments, and the only way to use it is to
keep reading ordinary runs of guests nobody is currently investigating. 461's table of blocked work
is still accurate; this pass says the answer to an idle bus is not to stop looking but to look
somewhere unrelated.

## Next

- Why the `obscene` eboot runs to the time limit - the question this pass set out to answer.
- The blocked list in 461, unchanged.
