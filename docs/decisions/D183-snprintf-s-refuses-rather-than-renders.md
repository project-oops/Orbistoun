# D183 - snprintf_s refuses rather than renders what it can


**decided** · 2026-08-21

The top-ranked finding for four sessions: seventy-six calls in a boot, nothing implementing
it, sitting between `sceKernelAllocateMainDirectMemory` and `sceKernelMapNamedDirectMemory`
with a stack address as its destination - the guest building the *name* the mapping call
takes.

### Partial rendering is invented data

The obvious implementation renders what it understands and leaves the rest. That produces a
string like `texture_` where the guest expected `texture_47.gnf`, which **is not a failure
the guest can detect**: it opens the wrong file, and the fault surfaces somewhere with no
connection to formatting.

So a format that cannot be honoured completely produces an empty, terminated destination
and a zero return. Also wrong, and wrong in a way that is bounded and immediate - the same
trade the knowledge file had already argued for while the function was a stub (principle 3).

### Floating point is not "unimplemented", it is unreachable

Under System V a variadic floating-point argument arrives in an **XMM register**, and the
trampoline captures the six integer registers. The value never reaches this function.
Rendering `%f` would emit a confident number derived from an unrelated register.

That earns its own variant rather than folding into "unsupported", because the two need
opposite responses: one is an hour's work in this file, the other is a change to the
calling convention. A report that said only "formatting failed" would send someone to the
wrong place.

Same for running out of arguments. Three fixed parameters leave three integer registers, so
a fourth conversion reads past the end of what arrived. The remainder went on the stack -
reachable, since the trampoline already captures the entry stack pointer for the ABI check
(D159), but not from the argument array alone.

### Implemented is not the same as answering correctly

A refused format still counts as a call that reached an implementation, so `standing` rises
while the guest receives an empty string. That is precisely the improvement-shaped
non-improvement this project keeps having to guard against, and nothing else in a run could
see it - so formatted writes are counted and reported, present whenever anything was
formatted so that "all honoured" is a measurement rather than a silence (D175).

### What the run then said

```
  standing 863 of 933 calls answered by an implementation (7% on stubs)   [was 85%]
  verdict  same     nothing moved
  formats  76 writes, all honoured
```

**The verdict is right and useless.** Distinct imports and total calls are unchanged, so
the headline measure reports nothing happened, while seventy-six calls moved from
placeholder to real answer. The call count cannot see this class of improvement at all;
`standing` and the retired finding are the only things that can - which is the argument for
D181 arriving on its own, one day later.

`all honoured` is the fact worth having: this title's formats need no floating-point
conversion and no more than three variadic arguments. Unknowable without running it, and
the two assumptions the knowledge file records - what the target returns on truncation, and
whether it terminates an overflowing buffer - remain unmeasured, because nothing truncated.

