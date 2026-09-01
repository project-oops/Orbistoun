# D249 - Nothing the guest calls answers with the base, through any channel


**decided** · 2026-08-25 · measured, 23 imports in 6.6 seconds

The last channel into `image+0xafc959` that can be varied from outside. Arguments were
exhausted; unwritten memory in every region and a reservation at the faulting address
changed nothing; return values of the seven *unimplemented* calls had been dyed, but the
implemented ones had not, because forcing a return changes what a function does rather than
only what it says.

`Axis::Return` sweeps all of them, two sentinels each, and looks for an offset that agrees
across both - not a fault that moved, because anything moves a fault. The sentinels sit at
`0x7000_0000_0000` and `0x7700_0000_0000` rather than the argument sweep's: a forced return
may be treated as a region base and indexed a long way into, so a low sentinel can land
inside something already mapped and produce no fault at all, which is a silent false
negative rather than a measurement.

**Twenty-two of twenty-three are indifferent** - byte-identical faults whatever they are
told to answer. The exception is `memalign`, whose answer is dereferenced at `+0x10`. That
is a real relationship and it is not this wall's arithmetic, which is `+0xfffe0`.

So the base does not come back through an argument of any call, nor as the answer to any
call. **What remains is not a call at all** - something at load time, or something the guest
computes for itself. That is a narrowing of the previous position, which still allowed an
implemented function to be the source.

The sweep cost six and a half seconds, which is the point worth keeping: this was left
undone for a long time on the assumption that it was an expensive intervention.

