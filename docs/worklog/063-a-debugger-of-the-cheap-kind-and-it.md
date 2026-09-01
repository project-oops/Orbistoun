# A debugger, of the cheap kind - and it named the parked bug in one run


The fault handler is a vectored exception handler, which means the operating system hands
it the entire register context and the option to resume. It was reading `Rip`, printing two
numbers, and throwing the rest away.

So it now also reports **which import was executing** and **the registers** - `rsp`, `rdi`
and `rax` on the first line, all sixteen in the trace (D158). The attribution is the part
that matters: an instruction pointer inside a placed region is the guest running its own
code, but outside every placed region it is *our* code running on the guest's behalf, and
then the last import the guest entered is almost certainly the one that faulted. Both
additions are allocation-free, because the import label is a `&'static str` from the table
rather than a formatted string.

It paid for itself immediately - D157, which had beaten me an hour earlier:

```text
read of 0xffffffffffffffff while executing at 0x7ff78c49007a
  (inside libkernel::sceKernelMapNamedDirectMemory)
  rsp 0x600000800d18 (stack+0x800d18)  rdi 0x600000800e08  rax 0x7fff0002
```

`rax` already holds `InvalidArgument`, so the function had reached a refusal and was
leaving. And a read of `0xffffffffffffffff` is not a dereference at all - it is what
Windows reports for a **general protection fault**, which is what a misaligned aligned-SSE
access raises. `rsp` is ≡ 8 (mod 16) deep inside a compiled frame where one that started
aligned would be ≡ 0.

The trampoline's own doc comment called this shot before any of it existed: *getting the
alignment wrong does nothing at all until some callee executes an aligned SSE instruction
against a stack slot, and then faults far from the cause.*

Which reframes the whole thing. It is very likely **not a bug in the mapping** - the
mapping is just the first import that does enough work (a mutex, a vector, a reservation)
for the compiler to spill a vector register to the stack. If that holds, it has been true
of every run this project has ever made, and it will affect every implementation added
from here.

Deliberately not fixed tonight: the next step is measuring `rsp % 16` on entry to a guest
call, which is a trampoline change, and hand-written assembly at the end of a long session
is how you turn one bug into two.

### On the "continue" half

The same handler can return `EXCEPTION_CONTINUE_EXECUTION`, which is the whole primitive a
survey mode needs - fix the cause, retry the faulting instruction. Retrying rather than
skipping is the important detail: it needs no instruction-length decoding, so it needs no
disassembler, which this project does not want for provenance reasons.

Not built, and one trap to record before it is: **continuing past faults breaks the
progress measure.** The fault position is this project's one signal that a change helped
(D080). A run that continued past three walls did not get further than a run that stopped
at the first - it answered a different question. Survey mode has to produce a *list of
walls* and no verdict, and it must never be the default.

