# Two hypotheses instead of one, and the oracle that could not tell them apart


`sceLibcMspaceMalloc` was answered with zero because D125 says so, and nothing was compared
against it. The reason nothing was compared is that the alternative could not be **said**: the
policy had `StubReturn` for what a function answers and `StubWrite` for a region delivered
through an argument, and only the second could produce a region - the wrong one for a function
that returns a pointer.

They are one concept. `StubRegion { via, bytes }` with `Delivery::{Argument(u8), Return}`, and
the service decides delivery when it resolves the base (D300). "Answer with a region" needed no
new diagnostic either: `ORBISTOUN_MAP` plus `ORBISTOUN_RETURN` at the mapped base is exactly
that, which is the same two-axes-together shape D283 was about.

The loop now runs both and keeps whichever reaches further. Pointed at the same title:

```
*** sceLibcMspaceMalloc answered the code the guest followed;
    zero reaches 25 against 13, and did not fault
```

**It kept zero, and the reason is the finding.** Zero already reached every import the run
makes and produced no fault at all, so a region had nothing to improve on. The comparison ran
and the oracle had no headroom to express a difference (D301).

`FURTHER` answers "did the guest get past something" - the right question while a wall exists
and no question at all once one does not. Two answers that both clear the last wall are
indistinguishable to it however different they are to the guest.

What would separate them is `035-libc`, which grades an allocator against a spec. That is
`Evidence::ConformanceCheck` - which every region-bearing patch already declares it needs and
**nothing enforces**. It is a label, not a gate, and this is the case that shows why the gate
has to exist before any of this runs unattended.

