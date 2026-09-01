# Call sites, aliasing, and a wall that has taken eight attempts


**Call sites** (D173). Principle 9 has always said the right question is *which call site*,
not which function - and the return address was never recorded. It was free: the trampoline
already carries the stack pointer as the guest's `call` left it, for the alignment check,
and the return address is the word sitting at exactly that address. One read.

It pays off immediately by joining the two traces:

```text
frames:      image+0xdbfb -> image+0xde58 -> image+0xdc4f -> ... -> image+0x43c4
call sites:  allocate from image+0xdfc1, snprintf_s from image+0xe021, map from image+0xe047
```

Same neighbourhood - **that cluster is the asset loader**. The chain then descends into a
different cluster and faults with no import call in between, so the fault is guest code
working on data it has just loaded rather than a missing function. Neither trace showed
that alone.

**Physical aliasing** (D174). `sceKernelMapNamedDirectMemory` ignored its physical argument
and handed out fresh memory every time - so a guest that maps, loads a file, and maps that
range again would read zeroes out of a buffer it had filled. Silent, total, and faulting
nowhere near the cause. Now keyed by physical offset.

It did not move the wall either, so that hypothesis was wrong too. Kept because the bug is
real whether or not this title triggers it.

**Eight attempts on `image+0x43c4` now**: filesystem, operator new, blanket ok, video
handles, snprintf_s, system-service out-pointer, and physical aliasing. What has come out
of it is three diagnostics that did not exist yesterday - frame chain, call sites, and the
register dump - and the fault is now precisely located rather than mysterious. The next
move is not another guess at a function.

