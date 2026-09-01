# D198 - Findings for the commonest outcome, and dumps for the ones already implemented


**decided** · 2026-08-24

Three defects in the finding machinery, all the same shape: **the run knew something and
did not say it.**

### A fault produced no finding at all

`diagnose` had a kind for a guest that stopped itself, for an unnamed import, for a
placeholder used as a pointer - and none for a guest that faulted, which is how nearly
every run in this project ends. The commonest outcome generated an empty work list.

`Gap::Faulted` classifies by arithmetic on the address and nothing else: zero is a null
dereference, below one page is a field read through a null pointer, a placeholder code is
one of ours used as an address, and anything else is an address in no region the run
mapped. Each of those is a *different mistake*, and the difference is exactly what a reader
was working out by hand every time.

It is weighted below every gap that names a function to fix. A fault says where the run
ended; a missing implementation says what to do about it.

### Implementing a function made its arguments invisible

Argument dumps were attached to unimplemented calls only, on the reasoning that an
implemented function needs no explanation. The reverse is true when the implementation is
suspected: `memalign` was the leading hypothesis for a wall for most of a session, and the
run could not show what it had been asked for. `ORBISTOUN_DUMP=<names>` forces dumps for
named functions regardless of whether they are implemented.

### Only pointees were recorded, so scalars vanished

A dump followed each argument and recorded what it pointed at. An argument that *is* a
value - a size, a count, a flag - pointed at nothing and was therefore absent. The wall
above was settled in one run once `arg0 = 0x8, arg1 = 0x1988` appeared: a succeeding
allocation request, which ruled the function out. Scalars are now recorded alongside
pointees.

**The handler is looked up once** and the dump decision made from that result. An
`is_implemented` call in the dispatch path costs nothing on the six calls being
investigated and a great deal on the sixty-eight million that are not.

