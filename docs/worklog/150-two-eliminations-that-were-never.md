# 2026-08-25 - two eliminations that were never measured


Went to the `image+0xafc959` wall to find where the region base comes back. Came away with
one new fact about the wall and two about the tools, and the second pair matter more.

**The diagnostics could not ask the question.** `ORBISTOUN_WRITE` planted one value at
offset zero of a structure with eight words in it, so three zeroed candidate slots were
three runs against three baselines. It now takes offsets and a list, so they are one run
with a distinct dye in each - and whichever the guest uses names itself (D229).

Building that found two more of the same shape: `ORBISTOUN_POKE` refused every stack
address, which is where the arguments worth poking live; and the memory diagnostics ran
*before* the stack fill, so a poke under both was erased by the fill and the run reported an
ordinary result.

**And one elimination in `PROJECT_STATUS.md` had never happened.** "The stub return" was
ruled out by a run in which nothing was overridden - `StubPolicy` is keyed by symbol name,
this function has none, so the override matched nothing and the default answered. No change
was the only result that run could produce. `ORBISTOUN_RETURN` now forces a 64-bit answer
and reaches a function by hash; under it the fault is still at `0xfffe0` with `rax=0`, which
is what the broken run said - the difference is `(1 answered)` in the conditions, proving it
ran (D230). The elimination is real now.

**The fault printed four registers of sixteen**, all of which were already captured and
written to the trace. Printing the rest immediately showed `rbx=0x20`, `r14=0x20`,
`r15=0x10` - the header offset in a register, every run for weeks - and narrows the missing
base to `rax`, `rsi`, `r8`, `r11`, `r12`.

### Where the wall stands

Eliminated, each with a count proving the diagnostic ran: all three zeroed slots of `arg0`
and `arg5` (`3 planted, 0 refused`), the return value (`1 answered`), and the one field of
the faulting object that stays zero all run. That last carries a caveat worth keeping - a
snapshot diff cannot tell "never written" from "written with zero", so a constructor
zeroing it would look identical.

Still open: which of the five zeroed registers should have held the base, and from where.

### The sweep the new plants made possible

With offsets and lists, the wall function's whole interface went in a handful of runs
instead of eight:

| Candidate | Result |
|---|---|
| `arg0+0`, `arg0+0x18` | inert - dyed, `3 planted, 0 refused`, fault unmoved |
| `arg0+8`, `arg0+0x10` | **inputs** - dyeing them kills the caller with an illegal instruction at `image+0x1595e75` |
| `arg5+0`, `+8`, `+0x10`, `+0x18` | inert - fault byte-identical to an ordinary run |
| return value | inert - `1 answered`, `rax=0` at the fault regardless |
| object field `image+0x19e9cb0` | inert - and the poke *survived*, proven by watching it |

That last row resolves a caveat rather than adding one. A snapshot diff cannot tell "never
written" from "written with zero", so poking a field and seeing no change is ambiguous -
unless the watch runs after the poke, in which case the field reading unchanged at the end
means the dye is *still there* and no constructor overwrote it. Combining the two
diagnostics answered what neither could alone.

**The conclusion is negative and firm.** The base does not come back through this call.
The allocator characterisation still stands - `arg1` is a size, `arg3` an alignment, and the
caller lays an arena header at `region_end - 0x20` - but "this is where the base is lost" is
now wrong. Something earlier should have established the region.

