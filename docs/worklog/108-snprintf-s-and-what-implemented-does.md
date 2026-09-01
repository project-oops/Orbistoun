# 2026-08-21 - snprintf_s, and what "implemented" does not mean (D183)


The top-ranked finding for four sessions, retired. Seventy-six calls in a boot, the guest
building the name that `sceKernelMapNamedDirectMemory` takes.

**It refuses rather than rendering what it can.** A half-rendered `texture_` where the guest
expected `texture_47.gnf` is invented data shaped like a real answer - the guest opens the
wrong file and the failure surfaces with no connection to formatting. An empty terminated
buffer is also wrong, and bounded.

Floating point earns its own fault variant rather than folding into "unsupported": a
variadic double arrives in an XMM register and the trampoline captures the six integer
registers, so the value never reaches the function. One of those is an hour's work in this
file; the other is a change to the calling convention. A report saying only "formatting
failed" sends someone to the wrong place.

**The run then made the D181 argument for me, one day later:**

```
  standing 863 of 933 calls answered by an implementation (7% on stubs)   [was 85%]
  verdict  same     nothing moved
  formats  76 writes, all honoured
```

Distinct imports and total calls unchanged, so the headline measure reports nothing
happened - while seventy-six calls moved from placeholder to real. The call count cannot see
this class of improvement at all.

`all honoured` is the fact worth having and could not have been reasoned to: this title's
formats need no floating-point conversion and no more than three variadic arguments. Both
recorded assumptions survive - nothing truncated, so what the target returns on truncation
is still unmeasured.

The wall is unchanged at `image+0x43c4`, which is now the ninth attempt on it.

**And the record refused to accept it**, which found a fourth bug of the same family.
`Status::beats` ranked on reach, imports and calls - the three numbers that did not move.
Implementing a function the guest was *already calling* moves no import and no call: the
guest makes exactly the same calls and gets real answers to more of them. That is the most
common kind of progress this project produces, and the compatibility record could not see
it, having been written the day after `standing` was invented for exactly that reason.

`standing` now ranks between imports and calls - breadth first, then how much of that
breadth is real. Every one of the four bugs found today came from running the tool rather
than reading it.


