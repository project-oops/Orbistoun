# 2026-08-24 - The unknowns become a queue (D196)


`orbistoun-cli questions` emits every recorded assumption ranked by how often a guest calls
the function, with `--json` for a probe or an agent. Sixty-seven questions; the top is
`sceKernelDirectMemoryQuery` at 87.6 million calls.

**Nearly built something that already existed, again.** `orbistoun-probe` reads obSCEne's
line protocol and maps a hardware answer to `Oracle::Measured` - built by another session
while this one was running, complete with captured transcripts so it is testable with no
hardware attached. The *return* path was done; only the outbound queue was missing. Checking
before proposing is now two-for-two on saving wasted work today.

Each entry carries `returns` and `arity`, which were recorded for trace fidelity and turn
out to be the dispatch key for a property: everything answering a handle can be asked the
same questions. That is what makes broad coverage tractable - a dozen property templates
over sixty-six functions, rather than sixty-six bespoke tests.

The queue prints `shape unrecorded` for the fifteen entries with no return kind rather than
omitting them. A function no property can dispatch on is itself a gap, and hiding it would
flatter the coverage.

### On the strategy question behind it

The techniques are not new - metamorphic, differential and property testing are decades old.
What is unusual is having an **enumerated queue of unknowns that exists before any symptom
does**. Traditional emulator work generates its queue by accident, in whatever order games
expose bugs; that is why it takes years. Retiring questions is a different activity from
chasing bugs.

Most of the classes worth catching need no oracle at all: a stub answers OK to inputs that
must fail, a bounded allocator returns an address outside the bounds it was given, a
set-then-get does not round-trip, a permutation changes the answer, an out-parameter's
poison pattern survives. None of those needs to know the right value - only that the
result contradicts its own inputs.


