# D393 - A capital letter is a different symbol


**assumed** - 2026-08-30

The conformance probe reported *the platform reports being both a devkit and a retail unit*,
which is the exact failure D271 fixed five days earlier - and D271's fix was in place and
correct.

It answered `sceKernelIsDevKit`. Every guest imports `sceKernelIsDevkit`, with a lower-case
`k`. A NID is a hash **of the name**, so those are two different symbols: the implementation
was never reached, the real import landed on a stub, and a stub answers this project's
placeholder - which is non-zero, which for a boolean reads as **true**.

The uncertainty was even recorded at the time:

> The vendor's own spelling is unsettled - a mined list carries both this and a lower-case k,
> and only one can exist on the platform. **A by-name lookup on real hardware is what settles
> it.**

It was settled by something better: a guest imported the hash, and this project's own naming
pipeline resolved it to `sceKernelIsDevkit`. A name is in that database only because it hashed
to a real import, so this is stronger evidence than the list that raised the doubt - `assumed`
becomes `guest-observed` without hardware being involved.

### Two more of the same shape, found in the same trace

`sceKernelIsNeoMode` and `sceKernelIsDevelopmentMode` were imported and unimplemented, so both
answered the placeholder, so both read as **true**. A guest was being told it was on the faster
hardware revision, in development mode, on a devkit, and on a retail unit.

### What the test was doing

Asserting the right answer about the wrong symbol, and passing. It called `sceKernelIsDevKit`
too, because the test and the declaration were written together from the same guess.

So there is now a second test that asserts the *property* rather than the values: every member
of this family must answer something small enough to **be** a boolean. A function dropped from
the table, or misspelled out of it, fails there rather than in a guest six weeks later.

