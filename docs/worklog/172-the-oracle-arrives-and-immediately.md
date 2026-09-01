# The oracle arrives, and immediately corroborates


The fix loop could generate changes and could not tell a good one from a lucky one. `FURTHER`
answers "did the guest get past something" and saturates the moment a run stops faulting
(D301), so a careful generator was being guarded by an oracle that could not refuse anything.

The naming loop has never had that problem. It brute-forces billions of candidates and has
never produced a wrong name, because a hash cannot be fooled. **The generator is dumb and the
oracle is perfect**, and that is why it runs unattended. The fix loop had it backwards (D302).

`orbistoun-turn::conformance` grades a change against the probe: 515 checks, each announced by
name, each graded against a spec. `037-math/sqrt` passing means sqrt is **correct**.

```
score before -> apply the candidate -> score after
keep if something went fail->pass and nothing went pass->fail
```

### What it said first time out

The `sceKernelReserveVirtualRange` patch - measured on `PPSA02664`, a commercial title - was
graded against the **conformance probe**, which is a different guest entirely:

```
graded: 2 check(s) now pass: 015-sync/thread-churn, 017-posix/rwlock
```

That is corroboration rather than absence of harm. A contract derived from one guest's
behaviour made two spec-graded checks pass on another. Nothing about the patch was tuned to
the probe; the probe had never been consulted when it was measured.

### Three rules the verdict holds

- **Per check, not per count.** A change that fixes one and breaks one has an unchanged total
  and is not an unchanged tree. The test asserts exactly that case.
- **A regression is a refusal, not a trade.** Nothing here can weigh one function's correctness
  against another's, so a change that fixes two and breaks one is refused rather than scored.
- **A check that stops running counts as broken.** A change that makes the probe die earlier
  produces a shorter report, and counting only what came back would read a crash as an
  improvement - the exact shape of every failure in this log.

### And the honest limit

A function no check exercises has no oracle. It stays `assumed`, and the gate says so rather
than quietly downgrading to reach. That set shrinks every time obSCEne gains a check - which
is the clearest argument yet for the probe being the place to invest.

