# D158 - The fault handler always had the registers; it printed two addresses


**decided** · 2026-08-20

A vectored exception handler is passed the full `CONTEXT` - every integer register - and
the ability to resume. This one read `Rip`, formatted two numbers, and discarded the rest.

Chasing D157 with nothing but "a host address faulted" was what made the cost obvious. So
the report now also carries:

- **Which import was executing.** An instruction pointer inside a placed region is the
  guest running its own code; outside every placed region it is *our* code running on the
  guest's behalf, and the last import the guest entered is then almost certainly the one
  that faulted. That single line turned "somewhere in the emulator" into one function.
- **The registers.** On the first line: `rsp`, `rdi`, `rax`. In the trace: all sixteen.

Both are allocation-free, because the import label is a `&'static str` from the table
rather than a formatted string - the part of the report that may allocate stays after the
part that may not.

**It solved D157 on the first run**, which is the only endorsement worth giving it:

```text
read of 0xffffffffffffffff while executing at 0x7ff78c49007a
  (inside libkernel::sceKernelMapNamedDirectMemory)
  rsp 0x600000800d18 (stack+0x800d18)  rdi 0x600000800e08  rax 0x7fff0002
```

`rax` already holds `InvalidArgument`, so the function had reached a refusal and was on its
way out. A read of `0xffffffffffffffff` is not a pointer dereference at all - it is what
Windows reports for a **general protection fault**, which is what a misaligned aligned-SSE
access raises. And `rsp` is ≡ 8 (mod 16) deep inside a compiled frame, where one that
started aligned would be ≡ 0.

The trampoline's own documentation predicted exactly this, before any of it was written:

> Getting this wrong does nothing at all until some callee executes an aligned SSE
> instruction against a stack slot, and then faults far from the cause.

So the hypothesis for D157 is now specific and testable: **guest calls arrive on a
misaligned stack**, every import so far has been small enough not to care, and
`sceKernelMapNamedDirectMemory` is simply the first one to do enough work - a mutex, a
vector, a reservation - for the compiler to spill a vector register to a stack slot.

That reframes it. It is not a bug in the mapping; the mapping is the first witness. If it
holds, it has been true of every run this project has ever done, and it will affect every
implementation from here on.

The next step is a direct measurement of `rsp % 16` on entry to a guest call, not more
reasoning - and that is a trampoline change, which is why it is not being made at the end
of a long session.

