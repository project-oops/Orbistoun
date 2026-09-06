# D469 - PPSA02664's wall is not `_Getpctype`; the trace was naming the wrong function

**measured, and CORRECTED BY [D470](D470-a-run-that-wrote-no-trace-reports-nothing.md)** - 2026-09-02
(user-directed /loop, continuing D468)

> **Read D470 first.** The measurements below are sound and still stand. The *conclusion* drawn from
> them - that the report mislabels the faulting call - is wrong. The report was not mislabelling
> anything: it was printing a trace from an earlier run, because the worker had stopped writing new
> ones. The title of this entry is left as it was written rather than quietly improved, because the
> mistake is the point.

`_Getpctype` is now implemented from measured hardware data (D468), and PPSA02664 faults in exactly the
same place with exactly the same message: `image+0xb14be3`, `read of 0x7fff00cf`, `rax=0x7fff0001`, and
a report line reading **`libc::_Getpctype was called 1 times and nothing implements it`**.

That line is false, and this entry is the measurement showing it.

## What was measured

Probes in the dispatcher and the label table (all removed; both files are byte-identical to HEAD):

- **`_Getpctype` is import index 54, it is bound, and it never takes the stub path.** A probe on
  `handler.is_none()` lists every index that answers a placeholder across a whole run: 154, 148, 86,
  149, 100, 160, 161, 159, 158, 156, 233, 2. **54 is not among them.** A second probe on the single
  return point - every call answering `0x7fff_0001` - never names 54 either.
- **The implementation really runs**: 415 calls in one run, each answering a real table pointer.
- **The last placeholder answered before the fault is index 148**, three times in succession, and
  index 148's label is **`Il2CppUserAssemblies::setenv`**.
- **`setenv` is declared nowhere in this repository.** Its sibling `getenv` is implemented; `setenv`
  was never added.
- The symbol has *two* label slots - 54 `libc::_Getpctype` (the import) and 853
  `resolved::_Getpctype` (the by-name stub, D365) - and neither answers a placeholder.

## The hypothesis this kills

`sceKernelDlsym` is not involved. The guest makes **zero** `dlsym` lookups this run - `dlsym` already
logs every distinct name it is asked for, and the log is empty. The ~141k `dlsym` calls that made the
two-routes theory attractive are an **aggregate across 65 runs of the whole corpus**, not this title.
A per-run number was read as a per-title one.

## What this costs

[D443](D443-ppsa02664-s-allocator-wall-was.md), [D449](D449-ppsa02664-regressed-to-the-allocator.md),
[D450](D450-ppsa02664-s-two-walls-are-a-thread.md) and [D459](D459-trace-records-what-a-call-answered.md)
all name `_Getpctype` as one of this title's two walls, and D450 built a thread-race model on top of
that pairing. The `image+0xb14be3` half of that model rests on a label, and the label is wrong. The
race D450 describes may still be real - it was measured from *fault sites*, which are not in question -
but "the `0xb14be3` branch is `_Getpctype`" is not, and D450's prediction that implementing
`_Getpctype` would push every run onto the allocator is untested rather than confirmed.

**This is the failure principle 3 already names, one level up.** A report that says "nothing implements
it" about a function that demonstrably ran 415 times is *reporting more than its measurement supports*,
which is exactly what the five tools in that section did. The dispatcher knows the truth -
`is_implemented(54)` is true - so the disagreement is between the trace's own two halves, not between
the trace and reality.

**The mechanism is not yet found**, and this entry deliberately stops short of guessing it. What is
established is the disagreement and its direction; whether the fault is being attributed to the wrong
recorded call, or the label table is consulted with the wrong index, is the next question. Naming a
cause here would repeat the mistake being recorded.

## What follows

`setenv` is the concrete candidate: unimplemented, called three times immediately before the fault,
answering the placeholder the guest then reads through. **Implementing it is an intervention, not a
diagnosis** (D224/D226/D227) - if the wall moves, that needs a second observation of a different kind
before it counts. And it is a candidate rather than a conclusion: the value dereferenced at
`0xb14be3` was not traced to its producer, only shown *not* to come from `_Getpctype`.

The wall did not move this tick, and nothing here claims progress.
