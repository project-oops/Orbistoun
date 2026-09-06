# 2026-09-03 - (/loop) One thread call implemented, one deliberately not

```
suites 124   tests 2008   clippy/fmt/identity clean
wall unchanged: 181 distinct, read of 0xa0 at image+0x1389269
calls 415,415 (exact) -> 415,355..415,357 (+/-2)
```

Ninth cron tick. The static trail on the wall ran out, so this closed two queued gaps instead
and states what the second one cost.

## The static trail ended

The memory-manager initialiser `0xf138a0` has **no `lea` and no data reference** anywhere in the
eboot - only the single direct call at `0xf269e2`. So the ordering is fixed in the binary and
there is no earlier indirect path to find.

The callback slot `0x1AA52B8` has **no `call [rip+d]` or `jmp [rip+d]` through it** either. The
installer takes its address (`lea rsi,[0x1AA52B8]`), so the table is passed **by pointer** and
invoked through a register - which means the caller cannot be found by scanning. The binding is
also correct: the eboot imports NID `0x9fdbed4fe0989d70` and the module exports exactly that at
`+0x13d5e90`, which is the frame the fault reports.

What remains needs hardware behaviour or far deeper module RE, so this tick went to queued work.

## The arities came from the run, not a header

Neither function is declared in orbistoun and neither is in the POSIX crate, so there was no
in-project arity to cite. The run report prints the arguments an unimplemented call received -
the method that fixed `sceKernelCreateEqueue` at two (D516):

```text
scePthreadSetaffinity    arg1 = 0x1ffb (and 0x1)          -> arity 2, a subject and a mask
scePthreadGetschedparam  arg1, arg2 = two stack addresses  -> arity 3, two 4-byte out-params
                         four bytes apart, leftovers after
```

## One implemented

`scePthreadSetaffinity` is accepted, citing the attribute form's own bargain: orbistoun does not
pin guest threads. **The difference from the attribute form is stated rather than glossed** - it
owns a block and can hand the mask back; this has nowhere to put it, so a guest that read one
back would not get it. Nothing observed does, and the moment something does this needs a
per-thread record rather than a wider `Ok`.

`Ok` and not the placeholder because a caller testing a scheduling call against zero reads
`0x7fff_0001` as *the affinity was refused* - a lie in the direction that stops a guest.

## One deliberately not

`scePthreadGetschedparam` keeps its arity and its two out-parameters recorded, and **no code**.
What it should *write* is not established: orbistoun keeps no per-thread scheduling record, so a
policy and priority would be invented. Unlike `sceVideoOutIsFlipPending`, where the answer
followed from a model already written down (D516), there is no model here to derive one from.

The reason is in the knowledge file, so the next reader finds a decision rather than a gap.

## The cost, measured

```text
before  415,415 exact, three runs
after   415355 415355 415356 415356 415356 415357
```

**Distinct stays 181 in all six and the fault is identical**, so the verdict - which keys on
distinct imports - is unaffected. The raw count is now +/-2. Likely because the guest creates 32
threads and a scheduling call it now sees succeed changes how long something spins; **stated as
likely because it has not been measured**. What was measured is which numbers moved and which
did not, six times - D488 once made this claim without checking distinct and was wrong (D519).

And the guest reached something new: `sceKernelAddUserEventEdge`, two calls, never seen before.

Decision: [D523](../decisions/D523-the-thread-affinity-setter-and-the-one-beside-it-that-stays-unimplemented.md).
