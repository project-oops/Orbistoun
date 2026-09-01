# 2026-08-25 - The wall moved, and the address had been right all along (D223, D224)


**Done.** Four cheap diagnostics - `BSS_FILL`, `WATCH`, `POKE`, `MAP` - and the first
`FURTHER` at `image+0xafc959` in weeks.

### The steer that changed the plan

Asked for *"as much info as possible from as many sources as it can, and reduce the debug
loop"* rather than one perfect tool. That reordering was the whole value: the plan had been
a hardware watchpoint next, and a **snapshot diff** turned out to answer the live question
better for a fraction of the cost.

A watchpoint says which byte, when, from which instruction - a debug register, a
per-platform API, an exception per access. A snapshot says which bytes ended up different -
one memcpy. For *"did anything ever fill this slot in?"* the second is the whole answer.

It produced the faulting object's shape in one run: three words written, one slot never
touched, and a host heap address stored in it that differs every run.

### Then `POKE` eliminated the obvious suspect

The untouched slot was the candidate for the missing base. Poked with two values; the fault
did not move - **and the watch confirmed the poke survived untouched**, which is the check
that makes it an elimination rather than a value quietly overwritten. Those look identical
from the fault address alone.

Seven eliminations, and every candidate for a lost base gone.

### Which left the reading nobody had tested

That **no base was lost**. `rcx = 0xfffe0` is `0x100000 - 0x20`, and a fault reported as
*"an address in no region this run mapped"* is exactly as consistent with a missing mapping
as with a bad pointer - and every tool built so far had assumed the second.

Reserving `0xf0000+0x10000`: **FURTHER**, 213 bytes on, to a genuine null at
`image+0xafca2e`. The address was right and the region was not there.

Two days of "somebody forgot to fill a slot", and the answer was "the memory was not there".
Worth remembering the shape: **when every explanation of a wrong value is eliminated, check
whether the value was right.**

### Three things that fell out on the way

- The address-space layer **refused** a reservation at `0xff000` and named the reason - the
  host's 64 KiB allocation granularity. A layer that quietly took what the kernel offered
  would have produced a run that reported success and answered a different question.
- `Verdict::Further` was **mislabelling itself**. It fires either for more interface or for
  the same interface with the fault further along, and said *"reached imports it could not
  reach before"* for both. This run hit the second: 23 imports before and after. So the one
  line this project steers by printed a falsehood directly above its own `(+0)` count. It
  says *"executed code it could not reach before"* now, pinned by a test built from this
  run's numbers.
- **Fourth thing this session reporting something its measurement did not support**, after
  the `found_by` gate, the ceiling comparison and the readable window.

### And the correction

I had called the watchpoint Windows-specific. The debug registers are **x86** - same
silicon and semantics on Linux, only the API differs - and this repository already
`#[cfg]`-splits that shape in `platform.rs`, `fault.rs` and `report.rs`. Ordinary here, not
exceptional; saying otherwise made the option sound worse than it is.

### Correction, one run later (D226)

The entry above concluded that `0xfffe0` was an address the guest expected mapped. **It is
not**, and the same tool said so on the next run.

Watching the mapped region shows the guest writing `{next, prev, .., count=1}` into it - a
circular list head with one node, an arena descriptor, whose lock is the mutex taken
immediately before the fault. It asks for `0x100000` bytes aligned to `0x40000` and lays
that header at `region_end - 0x20`. With the base lost, `region_end` is `0 + 0x100000`, so
the header lands at `0xfffe0`.

So the address was **wrong**, and the mapping gave a wrong pointer somewhere to land rather
than supplying a region the guest wanted. The `FURTHER` was bought by answering wrongly,
which is the progress principle 3 refuses to count - and the entry above flagged that exact
risk in its own text before drawing the opposite conclusion, which is worse than not
flagging it.

**What is genuinely gained:** the first *positive* fact about that function after seven
eliminations. It is a **region allocator** - `arg1` a size, `arg3` an alignment - and
implementing it is the fix.

**The lesson this session keeps re-teaching:** a diagnostic that makes a fault move is not
thereby a diagnosis. Ask what the guest *wrote*, not only whether it got *further*.

