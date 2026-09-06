# 2026-09-03 - (/loop) A fault now says what its registers point at, and the label is "None"

```
suites 124   tests 2007   clippy/fmt/identity clean
wall unchanged: 181 distinct, 415,415 calls
```

Eighth cron tick.

## The question, and the instrument that could not answer it

D521 left one thing open: why does the module invoke the callback? It takes a short text label -
`rbx=4`, `r12=0x6000007fc390` - so the answer was four characters at a known address, unreadable.

A watchpoint on that stack address reported eight sites, **every one a host address** (`0x7ff7...`):
orbistoun's own shims run on the guest stack and churn that slot, filling the thirty-two-site
recorder before the guest's own access appeared. A correct instrument, pointed at a question it
cannot answer.

## What was missing

The argument dumper has named and dumped pointers since D198 - **for imports only**. A fault
printed sixteen bare values and said nothing about any of them, which is the harder case,
because a fault is where a reader has least other information.

`FaultSite::pointees` now fills that in: for each register holding a **readable** address
(`is_mapped`, the same question the argument dumper asks), sixteen bytes and the region.
Registers that fail the test are omitted rather than reported as unreadable - fourteen "not an
address" lines would bury the two that are.

```text
rax -> stack+0x7fc2e0 = 4e 6f 6e 65 00 ...  "None"
rsi -> stack+0x7fc240 = 65 4e f1 22 ff ee 1f 3c ...
r12 -> stack+0x7fc390 = 4e 6f 6e 65 00 ...  "None"
```

## The answer points away from the hypothesis

**The label is `"None"`** - a memory-allocation scope name, and `"None"` is what a scope is
called when it has no name. The most ordinary value such a hook can take, **not an error tag**.

So D521's shape - that a module reports through this hook only on an unusual path - is weakened,
not supported. The module appears to be doing something routine, and the routine thing needs a
memory manager that does not exist yet.

## Quoting bytes as text is a place a report can lie

Three conditions, all required: something before the terminator, a **terminator inside the
window**, and every character printable. The middle one came from this run's own data - `rsi`
points at `65 4e f1 22 ...`, a pointer whose first two bytes are `"eN"`, which a looser rule
quotes as a string in the middle of a fault report.

Both halves broken and watched to fail. What the test cannot check is stated in it: a four-byte
integer whose bytes are printable and whose fifth is zero is indistinguishable from a short
string, and this reports it as one.

## Also measured

Only `sceKernelUuidCreate` of the five unimplemented functions is imported by the module at all;
the other four are the eboot's. Forcing all five to success was already shown not to move the
fault, so **an unimplemented import putting the module on an error path is now a weak reading**.

Decision: [D522](../decisions/D522-a-fault-now-says-what-its-registers-point-at.md).
