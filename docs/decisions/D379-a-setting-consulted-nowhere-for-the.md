# D379 - A setting consulted nowhere, for the fourth time


**decided** - 2026-08-29

Two things came out of putting the handoff structure where the runtime keeps it. The second
matters more than the first.

### payload_args holds what the entry point received

`payload_args` is the runtime's own global for the structure it was handed, and a run entering
past the runtime never received one - so the global held a marker. It holds the **same block
the declared-entry path is handed** now: resolver in field zero, markers in the fields nothing
has established.

That makes the two modes agree about what the guest is looking at, which they did not before,
and it means a field the syscall path reads says which field it is rather than saying nothing.

It did not move the wall. Recorded because it is right, not because it worked.

### The diagnostics had stopped reaching half the program

Forcing `vsnprintf` to answer a value did nothing. Not "the guest carried on and failed
elsewhere" - **nothing**, no report, no change, no sign that the knob had been read.

`ThunkTable::len` means the guest's own import count, deliberately: a report saying "1,410
import stubs, 254 implemented" must not quietly start counting everything this emulator can
answer (D366). Every *diagnostic* was sized by that number - forced dumps, forced writes,
forced returns - and by-name resolutions live past it. So the whole set silently excluded
exactly the calls the payload work is about, and said nothing about having done so.

**Fourth appearance of this shape.** D082: the registry consulted nowhere at the call site.
D166: the stub policy consulted nowhere. D187: an `ok` sweep whose functions never saw it,
from which "return values are not the cause" was concluded. Each time a knob existed, was set,
and reached nothing - and each time the run reported no change, which reads exactly like a
measurement.

The reports keep `len`; the diagnostics take `total`. Two numbers, two names, and the comment
at `prepare_diagnostics` says which is which and why.

### What it bought immediately

The argument dump fired for the first time and named the format `klogsrv` was rendering:

```text
%s:%d:%s: %s\n
```

- a file, a line, a function and a message: `klog_printf`'s own prefix. Four conversions, all
inside the register half of the list, so nothing walks into the overflow area.

Which leaves the `-1` read unexplained. Every pointer into that path is guarded now - the list
address, both areas inside it, the format, the destination, a `%s` argument - and the fault is
unchanged, at a fixed offset in this project's own binary, with the last recorded call being
`vsnprintf`. Six eliminations and no cause.

Saying that is better than a seventh guard. The next step is to look at where in *our* code it
faults, which is a thing this project can do to itself and has not needed to before.

