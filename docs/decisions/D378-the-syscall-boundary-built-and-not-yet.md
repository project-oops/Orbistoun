# D378 - The syscall boundary, built and not yet reached


**decided** - 2026-08-29

Everything in this project intercepts a guest at the *library* boundary: a relocation puts a
stub where a name was, and the stub says who was called. That is the whole of D005 and it
covers every commercial title measured. The open-toolchain payloads go **under** it, keeping a
pointer to a raw syscall gadget and calling it directly (D376).

So orbistoun has to be the kernel here as well as the library. This is that boundary, and an
honest account of how far it got.

### A number is a name the guest did not spell

`SYS_write` is four; `write` has been implemented here for a while. The mapping between them
is harvested from `sys/sys/syscall.h` in the checkout the ABI constants already come from,
so the numbers stay traceable to the header the way every other constant does.

Two things the rule cannot do on its own, and both are tables rather than cleverness:

- **`SYS___sysctl` is `sysctl`.** A rule with exceptions applied by code binds a number to the
  wrong function silently, which is the worst failure this boundary has.
- **`SYS_syscall` and `SYS___syscall` bind to nothing.** They are the indirect forms: the
  number they carry is *another* number, in the first argument. Binding them to anything would
  perform the wrong call with every argument shifted by one.

An unknown number answers `ENOSYS` negated - harvested, not written down - because a kernel
refuses and a stub that answered success would tell a guest its request was performed. Every
distinct number is reported once, known or not: reporting only the misses would hide the thing
worth knowing first, which is whether a guest reaches this boundary at all.

### The harvester was silently dropping the whole header

`is_constant_name` required upper case throughout, which is right for every header harvested
until this one - where every name is `SYS_read`, `SYS_write`, `SYS_getpid`. It took **one**
constant out of six hundred, and the only thing that said so was the count going up by one.

Names may be mixed case now. The count went from 1,094 to 1,538, so eleven other headers had
been giving up constants too.

### A gadget must clobber what `syscall` clobbers, and nothing else

`syscall` destroys `rax`, `rcx` and `r11` and **preserves everything else**. A System V
function call destroys all six argument registers. So a gadget that simply tail-called a Rust
dispatcher hands the guest back its own arguments as rubble - and the guest, having called
what it believes is one instruction, carries straight on using them.

The gadget pushes and pops the six, and the difference is asserted byte for byte, along with
the other thing that is easy to get silently wrong: the fourth argument comes from `r10`, not
`rcx`, because the instruction being stood in for destroys `rcx`.

### And klogsrv does not reach it

That is the honest part. With `ptr_syscall` holding a real gadget, `klogsrv` fails **earlier**
than it did with a marker there - inside this project's own `vsnprintf`, on a read of `-1`,
before any syscall is issued. The dispatcher's report confirms it: no number is ever asked for.

Which is D377's finding standing up rather than falling over. The guest **tests `ptr_syscall`
and branches on it**; setting it sends `klog_printf` down its syscall-available path, and that
path uses other globals - `payload_args` above all - that are still markers. Filling one thing
the runtime would have filled moved the program onto a road paved with the rest.

Four pointer guards were added chasing the `-1` and none of them was it: the list address, the
two areas inside it, the format, the destination, and a `%s` argument all now refuse all-ones
rather than dereferencing it. **They are worth keeping and they were not the cause.** Each one
turns a crash inside the renderer - where a report says `vsnprintf` and means something else
entirely - into a bounded refusal. What is left is a read this project has not accounted for,
and saying so is better than another guard.

### And a rule the fill needed

`klogsrv` has five separate globals called `calloc` and four called `strcpy`. A table of
resolved pointers would not, so those are statics that happen to share a name with a function,
and filling them writes a stub address into unrelated state. Only names that occur once are
filled now, and the count of the ones left alone is reported.

