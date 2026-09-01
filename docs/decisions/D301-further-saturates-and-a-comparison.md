# D301 - `FURTHER` saturates, and a comparison needs headroom to be a comparison


**decided** · 2026-08-26 · both answers were tried and the oracle could not separate them

D300 made the alternative sayable: a function whose answer the guest dereferences can now be
handed a **region** rather than zero, and the loop runs both and keeps whichever reaches
further. Pointed at `sceLibcMspaceMalloc`, it ran both and kept **zero**.

That is the right decision on the evidence and it is not evidence that zero is right. Zero
already reached every import the run makes and produced **no fault at all**, so there was
nothing left for a region to improve. The comparison happened and the oracle had no room to
express a difference.

**So the reach metric saturates**, and this is the first time that has mattered. `FURTHER`
answers "did the guest get past something", which is exactly the question while a wall exists
and no question at all once one does not. Two answers that both clear the last wall in a run
are indistinguishable to it, however different they are to the guest.

Three things follow.

**The comparison is still worth having.** It costs one boot and it separates the cases where
one option is strictly worse - which is most of them, and all of the ones where a rule would
have picked wrongly.

**What separates saturated options is the probe.** `035-libc` grades against a spec and would
say whether an allocator returned usable memory; reach cannot. That is `Evidence::
ConformanceCheck`, which every region-bearing patch already declares it needs and **nothing
currently enforces** - it is a label, not a gate, and this is the case that shows why the gate
has to exist before any of this runs unattended (D296).

**And a saturated verdict should say so.** A run that ends without a fault has stopped
measuring progress and started measuring nothing; reporting `FURTHER` from it reads as
confirmation. Not fixed here, but it is the same failure as every other one in this log -
reporting more than the measurement supports.

