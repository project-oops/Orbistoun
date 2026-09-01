# D300 - One concept: give the guest a region, and say how it arrives


**decided** · 2026-08-26 · the loop could hand a region to an argument and not to a caller

`sceLibcMspaceMalloc` was answered with **zero**, because D125 says a pointer-returning
function must not answer an error code and zero is what a caller may test. The guest accepted
it and got further, so the loop kept it and stopped.

It never tried the other answer. An allocator wants *memory*, and the loop already knows how to
reserve some - it does exactly that for an out-parameter. What it could not do was **hand one
back through the return value**, because the policy had two unrelated ideas in it: `StubReturn`
for what a function answers, and `StubWrite` for a region delivered through an argument. Only
one of them could produce a region, and it was the wrong one for a function that returns a
pointer.

They are one concept. *Give the guest a region, and say how it arrives*:

```toml
[measurement.region]
via   = { argument = 0 }   # write the base through arg0
bytes = 0x200000
```
```toml
[measurement.region]
via   = "return"           # hand the base back as the answer
bytes = 0x2000
```

So `StubWrite` becomes `StubRegion { via, bytes }`. The service resolves it to a concrete base
before the guest starts - as it already did - and then either installs a write or a stub
return, which is a decision about delivery rather than about behaviour.

**And it makes the alternative sayable, which is the point.** A rule that says "answer zero"
produced a result nothing compared it against. With both expressible, the loop can try each and
keep whichever reaches further - which is the exhaustive-rather-than-ranked discipline every
other sweep here already uses, absent exactly where a rule made it feel unnecessary (D231,
D299).

Renamed rather than added to, under principle 10: nothing has shipped, `learned.toml` has no
users beyond this machine, and carrying two names for one idea is how a vocabulary stops
meaning anything.

