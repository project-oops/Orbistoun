# D372 - Where the shared thing is what is under test, a lock is the fix


**decided** - 2026-08-29

`orbistoun-input`'s port table is process-global, because it describes one machine's
controllers. Two of its tests write to it: one publishes two pads and asserts both, the other
publishes one and asserts it replaced the last. Run in parallel, the second truncates the
table the first is halfway through checking.

It failed in the gate and passed on its own, which is the signature - and a gate that fails
once a day is a gate people stop reading, which is worse than the flake.

**Fifth appearance of this hazard**, after `orbistoun-abi`'s shared array, D323's fixed
addresses, the `.bss` fill cache and the format-fault counter. The fix has been *pass the
thing rather than reaching for it* every time before, and it does not apply here: the shared
table **is** what is under test. So this one takes the other fix - the exclusive guard
`orbistoun-fs` already uses for its descriptor tests - and the rule is now stated with both
halves:

> Where the shared state is incidental, pass it. Where it is the thing under test, serialise
> the tests that touch it.


