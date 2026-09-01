# D229 - A plant could reach only offset zero, and a fill silently erased it


**decided** · 2026-08-25 · found by needing it at the `image+0xafc959` wall

`ORBISTOUN_WRITE` planted one value, at the word an argument points *at*. The question at
the wall is which *member* of the structure the guest passes was left unfilled, and offset
zero is one member of eight. So the tool answered one candidate per run, each run a separate
comparison against a separate baseline - and the wall function's arguments have three zeroed
slots between them.

It now takes `<import>:<slot>[+<offset>]:<value>`, comma-separated. With a distinct value in
each candidate it is **one run**, and whichever value the guest uses names the slot it came
from. That is the reasoning behind the self-identifying memory-query fields (D220) pointed at
structures the guest passes rather than ones orbistoun fills in.

A malformed clause refuses the whole list rather than planting the rest. A half-applied
experiment records conditions describing what was asked for instead of what happened, which
is the failure this family of diagnostics exists to avoid.

**Two bugs found by building it**, both of the same kind - a diagnostic that quietly did not
run:

- `ORBISTOUN_POKE` refused any address outside a writable *image* segment, so every stack
  address was out of reach. The arguments worth poking at a wall are stack structures, so
  the restriction excluded the case the tool exists for. Any writable mapping is now
  allowed; read-only text is still refused, which was the actual point.
- `apply_memory_diagnostics` ran *before* `stack.fill`, so a run under both had its poke
  overwritten by the fill and reported an ordinary result. Now after, which also makes the
  watch snapshot capture the state the guest actually starts from.

**Result at the wall.** Dyes in all three zeroed slots, `3 planted, 0 refused`, fault
unmoved at `0xfffe0`. The three are eliminated - and this time the counts prove the
experiment ran.

