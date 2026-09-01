# D384 - A gadget is not reached from a call site a compiler wrote


**assumed** - 2026-08-30

An access violation at **`0xffffffffffffffff`** survived a dozen eliminations across two
sessions. It is not a wild pointer. It is what Windows reports for a **general-protection
fault**, and the instruction that raised it was this:

```text
0f 29 45 d0     movaps [rbp-0x30], xmm0
```

`movaps` is an *aligned* SSE store. `rbp` was `0x600000800c58`, so `rbp-0x30` was eight past
a sixteen-byte boundary, and the processor refused it. The optimiser had vectorised the copy
of six saved argument words into the dispatcher's own frame - a frame built on a stack that
was eight off.

### Why only the gadget

Everything else a guest calls in this project arrives through a **call site a compiler
wrote**: a relocation put a stub at an imported name, and the guest's own code called it with
the stack the ABI requires. The run reports as much on every boot - *all on a conforming
stack* - and that report has been true and has been checked.

A gadget is not reached that way. The guest holds a pointer where a real system holds
`syscall; ret` and goes through it however its own code happens to, and `ftpsrv` arrives
**eight off**. Nothing was wrong with the guest: a raw gadget has no ABI, so there is nothing
for it to be wrong about.

The arithmetic in the gadget was right for a conforming caller and had a comment explaining
why, which is how it survived: *the guest called us, so `rsp` is eight past alignment*. That
sentence is an assumption about the guest, written as though it were a fact about the
instruction set.

### What it does now

```text
push rbp          the guest's, and where the old stack goes
mov rbp, rsp
and rsp, -16      aligned, whatever it was
...
call r11
mov rsp, rbp      back to the guest's own, however it was aligned
pop rbp
```

`rbp` is callee-saved in both conventions, so the dispatcher hands it back. The same change is
made to the reporting gadget stubs, which had the identical hazard by the identical route and
had simply never been pointed at a guest that arrives misaligned.

### The general form

**A boundary this project controls one side of must not assume the other side's ABI.** It is
the same shape as D381 - the dispatcher runs on the guest's stack, so it must not allocate -
and this is the other half of it: the dispatcher runs on the guest's stack, so it must not
assume it is aligned either.

`ftpsrv` reached its own `main` and ran to completion the moment this changed, and every
payload that keeps a syscall gadget was one misaligned call away from the same fault.

