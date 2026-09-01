# D388 - A set says which and never when, and *when* was the whole question


**assumed** - 2026-08-30

The syscall record was a bitmap: one bit per number, so a run could say *`ftpsrv` asked for
20, 601 and 616* and nothing else. Which is the wrong half. Whether those come before or
after `Unable to change AuthID` is the difference between **the privilege path uses them** and
**the give-up path does**, and the answer decides whether there is anything to implement.

So the first sixty-four are recorded in order, with the first argument, into a fixed array of
atomics - allocation-free and lock-free, because this is the dispatcher and it runs on the
guest's stack (D381). The report says how many there were in total as well as how many it
kept, because a list that stops without saying so reads as a complete account (D385).

```text
orbistoun: the guest made 3 syscalls, in this order:
    0     20  getpid  (0x600000800ed0)
    1    616  nothing here implements it  (0xc58c)
    2    601  nothing here implements it  (0x7)
```

### What it settled about `ftpsrv`

All three are on the **give-up path**. The gadget that made the first one was called from
`main+0x226`, which is the instruction after the `puts` that printed the failure - so the
privilege attempt itself made no syscall at all.

It also does not *dereference* the kernel addresses. Run with them holding markers, `ftpsrv`
prints the same failure and faults on nothing; run with them null, the same. It checks
something else - the primitive it would perform the read *with* - and correctly concludes it
does not have one.

**So there is no syscall, import or global that moves `ftpsrv`.** Its escalation is
self-contained guest code that needs a working kernel read/write primitive, and it detects the
absence properly rather than crashing on it. D382 said this was a wall worth having; this is
the same conclusion reached by measurement instead of inference, which is the difference
between believing it and knowing it.

### What it showed about `elfldr`

`elfldr` and `pldmgr` have no `main` symbol, so neither can be entered past its runtime, and
both die inside `__crt_start`. Three things are now known about what it wants:

- **It resolves its C library by name, and this project's resolver already matches.**
  `sceKernelDlsym(1, name, out)` is the **three-argument** form - a handle, a name, and a
  pointer to write the answer through - which is what `orbistoun-kernel` implements. It
  resolves exactly two names before it fails: `sceKernelDlsym` itself, bootstrapping, and then
  `getpid`. So the resolution path is not the wall.
- **Field two of the handoff structure is a pointer that must be readable.** Under `zero` the
  run faults reading `0x0` at `image+0x682f` and under `strict` it faults on the field-two
  sentinel at the same instruction; with the region mapped it gets past that instruction
  entirely. Field two points at something, and the runtime reads it.
- **The wall past it is not the handoff structure.** With field two's referent mapped, the run
  dies in `pthread_rwlock_init` writing to `0x2001` - and it is `0x2001` under **both** marker
  schemes, which fill that region with completely different values. A number that does not
  change when its supposed source does is not from that source.

  It is also a real import call rather than a marker being called: `elfldr` imports
  `pthread_rwlock_init` in the ordinary way and the loader resolved it. So the guest computes
  a bad lock pointer out of something that is *not* what it was handed.

That is three facts more than yesterday and still not a layout - and the third **rules out**
the obvious next move rather than supporting it. Walking field two's referent member by member
would have been the plan; the evidence says that referent is not where `0x2001` comes from, so
the next session should start by finding what does, not by extending the marker depths.

