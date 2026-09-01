# D232 - A fault that moves and a guest broken earlier are the same address and opposite results


**decided** · 2026-08-25 · found by running the sweep this generalises

`orbistoun-cli env` lists ten diagnostics. Only one - planting a value at an argument - had
ever been swept automatically. `orbistoun-propose::axis` makes the rest sweepable on the
same terms, and the first run of it produced a result that read as a lead and was not one.

Poisoning zero-initialised statics **moved the wall's fault** from `0xfffe0` to a different
address entirely. Reported as `MovedTo`, which is what it was. The second observation says
what it means: the guest reached **8 distinct imports instead of 23** and died somewhere it
had never got to before. The poison broke it long before it came near the question being
asked. Not a lead - a regression wearing one's clothes.

So `Change` carries two signals, not one. `BrokeEarlier` holds the address *and* how far the
guest got, and `is_notable` is false for it, so it is never offered beside a real lead. D129
records the identical lesson about the progress verdict - one signal hid a run that had
reached eight more subsystems behind an instruction pointer that had gone backwards - which
makes this the second time the same mistake has been made in this repository from a
different direction.

`NotApplied` stays distinct from `Nothing` for the reason it was introduced: a run that
changed nothing because it *did* nothing is not evidence. That distinction earned itself
again immediately - the dispatcher's first live run swept the *region* a fault landed in
rather than the call that led there, planted nothing across every slot, and would have
reported a clean negative. See D233.

**The negative result, recorded because it is one.** Against the live wall, neither
uninitialised memory in any region nor a reservation at the faulting address changes
anything. Stack and heap fills were already known by hand for one title; heap, bss and every
`Map` reservation had never been asked at all. They have been now.

