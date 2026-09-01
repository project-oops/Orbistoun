# D366 - sceKernelDlsym is how a payload gets its C library, and the answer is a stub we already had


**decided** - 2026-08-29

D365 found that a payload's runtime is handed a resolver and asks it for names. This is what
answering properly took.

### The problem an import table cannot solve

A dynamic import is a name the linker resolved before the program started. The payloads
barely use one: `klogsrv` carries `vsnprintf`, `snprintf` and `sprintf` as eight-byte
**objects in `.bss`**, filled at startup by asking for one name at a time. Those names are in
no import table and no relocation, so nothing indexed by symbol index can answer them.

This also settles what D359 half-saw. The null the payloads jump to *is* a `.bss` global -
and it is `.bss` because the runtime was supposed to have filled it and never ran.

### The answer is the stub that already existed

The thunk table grew a second population: after the guest's own imports, one stub per
implemented name, with a name-to-address map published beside them. A lookup answers **the
same address the linker would have written**, so a function reached by name and the same
function reached by an import are one address, with one counter, one trace entry and one
implementation. A resolver that minted its own answers would have created a second way for a
call to behave, and the first divergence would have been unattributable.

One table rather than two, because dispatch is indexed by a single number and a second table
would need a second trampoline, a second counter array and a second way to be wrong.

### Three things that had to stay true

**A stub count still means imports.** `len()` is the guest's own; `total()` is every stub.
A report saying "1,410 import stubs, 254 implemented" must go on meaning the guest's, not
that number plus everything this emulator can answer - so `implemented_count_within` takes
the import count and the summary passes it.

**One list, walked twice.** The binding says slot `imports + n` is `resolvable()[n]`; the
call trace says the same thing in a different function. If the two orders could differ, a
resolved call would be attributed to a different function than the one that ran - which is
worse than no label, because it reads as evidence. There is now one list and a test that it
is stable.

**`unknown` is a label that cannot be looked into.** Two different unlabelled stubs read as
the same thing, so a trace showing both looks like one function called twice. It is
`unknown#<index>` now, and that immediately distinguished two calls this session that had
looked like one.

### What the guest then said

```text
the guest asked for the address of sceKernelDlsym - answered 0x7000000004a0
the guest asked for the address of getpid - answered 0x700000001580
```

Every distinct name, once, answered or not - the same shape as the `sysctl` report and for
the same reason. A payload's resolution pass is the clearest statement it ever makes of what
it needs, and it makes it before doing anything else, so printing only the failures would
throw away the half that says what the runtime is built out of.

`klogsrv` now runs `__crt_start` into `__kernel_init` and stops there, on a field of the
handoff structure that is still a marker. That is two walls further than this morning, and
the next one is a measurement rather than a mystery.

