# D380 - A fault in our own code should say where in our own code


**decided** - 2026-08-29

A fault report names the last import called and calls it *inside* that function. That is an
**attribution, not a location**: it says which function the guest wanted, not which instruction
faulted. For a fault in guest code it is the whole story, and this project never needed more -
a fault in the emulator's own code was somebody's bug to find by reading.

It stopped being enough when an implementation grew complicated enough to fault inside itself.

### Naming the site, on a toolchain with no symbols

The binary carries none, so an address in orbistoun's own code is opaque, and it is randomised
per run so it cannot even be written down. But a `GuestFn` is a function pointer and a function
pointer is an address - **this project already has a table of where every implementation
starts**. Sorted once at startup, a fault address finds the nearest preceding one.

The distance is printed with the name, and that is what makes it honest: a fault inside a
library routine the compiler called - a copy, a formatter - names the last implementation
*before* it. A small offset is a strong hint; a large one is visibly not a match.

Its first answer was the second kind, and useful for it:

```text
in orbistoun's own code, nearest implementation is posix_sigdelset+0x391c1
```

Two hundred kilobytes past the last implementation in address order. So the fault is **not in
an implementation at all** - it is in support code called from one, which is a fact the report
could not previously state.

### And the renderer now follows a pointer only where the run mapped one

The dumper has always checked before dereferencing an argument. Nothing else did: the C
library follows a guest pointer because *"a guest that passes a bad pointer faults here
precisely as it would have faulted there"*.

That is right for a pointer the guest **computed** and wrong for one it **never set**. A `%s`
whose argument came out of an overflow area holding no arguments is the second kind - arbitrary
stack contents - and no amount of guarding individual impossible values catches it, because the
value is arbitrary. Guarding null, then all-ones, then a third thing is a losing game.

So `%s` asks the same question the dumper asks, through the same published ranges, and renders
`(unmapped)` for a pointer outside them. Permissive when nothing is known: a caller outside a
run has no ranges to check against, and there the old behaviour is the right one.

This also removes an invention. The previous attempt rendered the literal text `(bad pointer)`
for an all-ones argument, which is text this project made up appearing in a guest's own output;
`(unmapped)` is a statement about a range this run published, which is a fact.

### What is still not known, said plainly

The `-1` read that started all this is **unexplained after eight eliminations** - the list
address, both areas inside it, the format, the destination, a `%s` argument, the stale-binary
class, and both pointer-following paths. The fault is unchanged.

What is now known about it: it is in support code, not an implementation; it is reached from
the `vsnprintf` path; and pinning it further needs a debugger or a symbolised build, neither of
which this toolchain gives. Every change made while chasing it stands on its own merits, and
none of them was the cause. Recording that is the point.

