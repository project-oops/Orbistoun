# 2026-08-29 - The syscall boundary, built and not yet reached


`klogsrv`'s last wall was a raw syscall gadget, so orbistoun grew a syscall boundary: numbers
harvested from `sys/sys/syscall.h`, a dispatcher, and a gadget with the convention a `syscall`
instruction actually has (D378).

**The harvester had been dropping headers in silence.** `is_constant_name` required upper case
throughout, and every syscall is `SYS_read`, `SYS_write`. It took one constant out of six
hundred, and the only thing that said so was the total going up by one. Fixing it took the
table from 1,094 constants to 1,538 - eleven other headers had been giving up names too.

**A gadget must clobber what `syscall` clobbers and nothing else.** The instruction destroys
`rax`, `rcx` and `r11` and preserves the rest; a System V call destroys all six argument
registers. A gadget that tail-called a Rust dispatcher would hand the guest back its own
arguments as rubble, and the guest - having called what it believes is one instruction -
carries straight on using them.

### And klogsrv does not reach it

With `ptr_syscall` holding a real gadget, `klogsrv` fails **earlier** than with a marker there,
inside this project's own `vsnprintf`, before any syscall is issued. The dispatcher reports
every number it is asked for, known or not, and it is asked for none.

Which is D377 standing up rather than falling over: the guest tests `ptr_syscall` and branches
on it, so setting it sends `klog_printf` down its syscall-available path - and that path uses
other globals, `payload_args` above all, that are still markers. Filling one thing the runtime
would have filled moved the program onto a road paved with the rest.

Four pointer guards were added chasing the `-1` and none of them was the cause. They are worth
keeping - each turns a crash inside the renderer into a bounded refusal - and they were not it.
Saying so is better than adding a fifth.

**Next**: `payload_args`. It is the handoff structure's own global, it is a marker, and the
syscall path reads it. That is the same question this has been circling all day, now reachable
from a second direction.

