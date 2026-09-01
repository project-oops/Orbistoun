# D377 - A syscall gadget is not a function


**decided** - 2026-08-29

D376 ended on `ptr_syscall`: `klogsrv` renders its message with `vsnprintf` and then calls a
raw syscall gadget. Watching that call needed a stub unlike every other one here.

### Every other diagnostic reports arguments, and that is not enough

The handoff reporters shift the guest's arguments along and print three of them, which is
right for a function. **A syscall gadget is not a function.** The number it is being asked to
perform arrives in `rax`, and the fourth argument arrives in `r10` rather than `rcx`, because
that is what the `syscall` instruction uses. No argument-shaped report can see either.

So a gadget stub saves `rax`, the six argument registers and `r10` into its own buffer and
prints all eight. Between them they say which convention the caller used, which is the whole
question. One buffer per stub, so two guest threads calling two gadgets cannot overwrite each
other's report.

### The measurement moved the thing it was measuring, twice

**First, by renumbering.** Markers were numbered by how many markers had been issued, so
pointing *one* global at a stub renumbered every marker after it - and the guest computes with
those values (D368). Numbered by the global's own position now, so watching one changes
nothing about the others.

**Second, and this is the finding.** With `ptr_syscall` holding an unmapped marker, the run
reaches it and stops there, reproducibly. With it holding a *stub* - an address that is real
and executable - the run fails **earlier**, inside `vsnprintf`.

So the guest **tests `ptr_syscall` before using it**, and takes a different path when it is
set. That is worth more than the register dump would have been: it says a stub answering zero
is not enough. A gadget that exists must work, because a program that finds one there proceeds
as though syscalls are available.

### What that means for the next unit

The syscall boundary has to be implemented rather than stubbed. That is bounded work and
nothing about it is a mystery: the numbers are in `sys/sys/syscall.h` in the checkout the ABI
constants already come from, the convention is FreeBSD's, and the implementations the numbers
map onto are written already - `read`, `write`, `open`, `close`, `getpid` and the rest have
been here for a while under their names.

What is *not* free is that a syscall dispatcher is orbistoun being the kernel, and every
number it does not know has to fail the way the kernel would rather than the way a stub does.

