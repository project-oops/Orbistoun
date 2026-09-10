# D656 - The context has to be on a real stack

**Status:** measured
**Date:** 2026-09-09

## The measurement

`REQ-20260909T1720Z-4e17` asked what the console hands a signal handler in `rsi`. Sweep
20260909-175052:

| | |
|---|---|
| `rsi` and `rdx` | the **identical** pointer |
| the pointer | `0x7eeffaeb0`, on the interrupted thread's own 2 MiB stack |
| its mapping | `[0x7eedfc000, 0x7eeffc000)`, confirmed by `sceKernelVirtualQuery` |
| `+0x48` | `1e00000000000000` - the signal number |
| `+0xf8` | `0x7eeffb698` - another address on that same stack, mapped |
| `+0x100..+0x170` | register and thread frame state |
| `+0x00` | zero |

## Three attempts, and the third is the finding

**A reserved page of orbistoun's own.** `0x5E2E_0000_0000`, per the address map's own rule for
regions this project invents. The handler ran past `mov rax, [rsi+0xf8]`, made ten more calls,
reached two more imports - and faulted reading `+0x13c8`, past the end of a 4 KiB page.

**The same region, 2 MiB - the measured extent of the console's own mapping.** It faulted reading
`0x368` past the end of *that*.

**That is the finding.** The guest is not reading a structure, it is **scanning** - forward, in
eight-byte steps, until it leaves the allocation. A root scan, which is what a collector does to a
suspended thread's stack. A scan is bounded by the allocation it is in, so it does not matter how
large an invented region is; it matters that it is the *right* allocation.

## So the context is built on the stack the handler runs on

`call_guest` already reserves a fresh guarded stack for a reentrant call and releases it after.
That stack is the closest thing this process has to the console's arrangement, where the frame is
built on the interrupted thread's stack and the handler runs on it. `call_guest_placing` hands the
caller that stack's span and takes the three arguments back, so only the function that owns the
reservation ever knows the address - it cannot be handed out early or kept late.

An earlier attempt used the *target thread's* recorded stack, which is `None` for `main` - it is
adopted rather than spawned - so the handler got a null and the run went straight back to where it
started. The failure was silent in exactly the way an `Option` fallback usually is.

## What it moved

```text
before:  141 imports   310,987 calls   ran to the time limit, silent for 19.7s of 20
after:   151 imports   321,973 calls   image+0x17554a3
```

**FURTHER** - the project's one measure of progress, and the first time this title has produced
one since the binding fix. The fault has left the title's own modules entirely.

Two runs agree on the fault site and on the import count; the **call count now varies by a few
dozen between runs**, which it did not before. That is expected and worth stating: a collector is
now actually running across threads, so the number of calls in a wall-clock second is no longer
fixed. The fault site is what D080 reads progress from, and that is stable. The frontier records
one call count and may need the call budget for this title if the jitter grows.

Nothing else in the corpus moved.

## What is still zero, and why it stays zero

`+0x100..+0x170` is register and thread state on the console. Orbistoun has none to put there: the
handler runs because a *different* thread raised the signal, and the interrupted thread's registers
are not this process's to read - it is parked inside a Rust condition variable, not stopped at a
guest instruction. Plausible values there would let a guest resume onto them. Zero makes it read a
null and check, the same trade the zeroed data blocks make (D323).

The inner pointer at `+0xf8` points `0x7e8` further along the same stack, which is the measured
relationship. What it points *at* is zero, because on the console it is whatever that thread
happened to have there.
