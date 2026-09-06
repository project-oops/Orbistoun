# D488 - The title's modules were placed on the guest's own heap arena

**measured** - 2026-09-03 (eight runs before and after moving one constant)

D487 recorded that a run of PPSA02664 is not reproducible: two states, alternating, producing
`FURTHER`, `BACK` and `same` on identical code. **This is the cause of the larger half of it,
and it was introduced four worklogs ago by the module loader.**

## What the two states were

The states differ by exactly one allocation round:

| | 68-distinct state | 69-distinct state |
|---|---|---|
| `sceKernelMprotect` | absent | 1 call |
| `sceKernelAllocateMainDirectMemory` | 14 | 13 |
| `sceKernelMapDirectMemory` | 14 | 13 |
| `sceKernelDirectMemoryQuery` | 4 | 3 |
| `sceKernelSetVirtualRangeName` | 14 | 13 |

Four fewer calls plus one `mprotect` is the -3 the report showed. The guest either does a
fourteenth allocation round, or thirteen and then re-protects.

## The cause, and where it was already written down

`TITLE_MODULE_BASE` - where D482 places the modules a title ships - was
**`0x5000_0000_0000`**.

**D443 already records that address as PPSA02664's.** Its C++ allocator reserves its heap arena
at exactly `0x5000_0000_0000`, which is the hint `sceKernelReserveVirtualRange` receives, and a
policy region placed there once already cost a null-pointer fault: the guest's reservation fell
back, its arena-relative size arithmetic underflowed, `tlsf_add_pool` rejected the pool, and the
next allocation returned null.

That decision moved the *policy* arena off `0x50…`. Then the module loader put the *modules*
there, because the address looked empty and the decision log was not read.

Moved to `0x4800_0000_0000`. Eight runs after:

```text
69 distinct, 10884 calls      69 distinct, 10884 calls
69 distinct, 10884 calls      69 distinct, 10887 calls
69 distinct, 10884 calls      69 distinct, 10887 calls
69 distinct, 10887 calls      69 distinct, 10884 calls
```

**Sixty-nine every time.** The distinct-import oscillation - the half that moved the verdict -
is gone.

## It also corrects a claim made one worklog ago

Worklog 337 reported *"+18 calls against a ±3 spread"* from linking the title's modules, from a
properly interleaved A/B. The measurement was right and the attribution was wrong: with the
collision removed, the linked path answers 10884-10887, which is the *unlinked* path's range.

**The +18 calls were the collision, not the linking.** Linking the modules has no measured
effect on how far this title gets - which is the honest result, and not the one that was
written down.

## What is not fixed

A **±3 call oscillation remains** across the eight runs above. It does not move the verdict,
which keys on distinct imports, but it is still a run that does not reproduce and its cause is
unknown. D487 stands for that remainder.

> **Both halves of that sentence were wrong, and D513 has the cause.** The ±3 travels with a
> ±2 in *distinct* imports, so it does move the verdict - the 2080 branch reaches three
> imports the 2077 branch never does and the report calls it `FURTHER` when both fault at the
> same instruction. It was invisible here because only one of the two branches could be
> produced on purpose. The cause is the guest's own allocator deciding between extending a
> range it holds and taking a fresh direct-memory segment, on the relative position of the
> block `malloc` gave it - which the host heap settled differently on every run.

## Three hypotheses this killed on the way

Recorded because each looked plausible and each was wrong, and the next reader should not spend
the time again:

- **Thread scheduling.** The guest never calls `scePthreadCreate` - it is single-threaded at the
  point it dies. `scePthreadSelf` and the mutex calls are there; thread creation is not.
- **Reservation failures varying.** Thirteen fail on every run, in both states.
- **The reservation failures being a bug at all.** They are the guest reserving *inside* a region
  policy handed it: a policy region plants its base into a guest argument, the guest's allocator
  reserves ranges at that base, and orbistoun already holds it. Confirmed by moving
  `POLICY_REGION_BASE` twice and watching every failure follow it. Expected, and now said so in
  a comment rather than rediscovered.

## The instrument that made it findable

`orbistoun_mem::reserve_failures()` and `first_reserve_failure_base()` were added for this. The
report had shown only the **last** failure, so two runs failing thirteen reservations each
looked identical to two runs failing one - and "did this run fail more than that one" was not a
question the report could answer. It is now.

Thirteen failures all at one base, with different lengths, is what turned a guess into a
mechanism.
