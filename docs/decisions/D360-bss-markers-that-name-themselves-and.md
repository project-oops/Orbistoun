# D360 - `.bss` markers that name themselves, and what they found


**decided** · 2026-08-29

[D359](#d359---entering-at-main-skips-the-initialisation-the-program-needed) established
*that* both payloads depend on `.bss` they were never given - a constant fill changes the
fault completely. It could not say **which** global, and "an unknown number of globals" was
the reason that route looked worse than working out the handoff structure.

So the fill carries markers instead: each eight-byte slot holds the fill byte in its top
byte and its **own address** in the rest. A guest that loads a global and uses it as an
address then faults on a value that reads back as `bss+offset`, and one boot names it.

### It has not fired, and the honest report is that

Neither payload calls an uninitialised global. Both **read** `.bss` and derive something
else - the fault is `read of 0xffffffffffffffff`, and a constant fill produces the identical
one, so the markers added no evidence here.

What they did narrow is where. Both land at the same place:

```
klogsrv  image+0x28fc   klog_printf +300
ftpsrv   image+0x819c   klog_printf +300
```

**Same function, same offset, two different programs.** So the dependency is in the SDK's
shared logging helper rather than in either payload - which is consistent with everything
else here pointing at the runtime rather than at the servers.

`klog_printf` needing something `__crt_start` sets up is a coherent story and is **not
established**: the mechanism between a filled `.bss` and a dereference of `-1` was not
determined, and working it out needs the payload's own code.

### Why the mechanism is tested even though no guest tripped it

A marker that never fires and a marker that is wrong look identical from a run. The unit
test decodes each slot back to the address it occupies, which is the difference between a
mechanism that is right and one that is merely present - the same reason every other guard
here has a negative test.

### A test that broke two others

The first version set the fill byte through the cache `bss_byte` reads once. Tests share a
process, so it changed what every other test in the binary saw and broke the one asserting
`.bss` is zeroed. The byte is a parameter now.

Third time this hazard has appeared - after the `orbistoun-abi` shared array and the
fixed-address collisions of D324 - and the fix is the same each time: **pass the thing
rather than reaching for it**.

