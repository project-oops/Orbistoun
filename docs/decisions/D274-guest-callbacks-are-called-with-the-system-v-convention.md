# D274 - Guest callbacks are called with the System V convention

**Status:** decided
**Date:** 2026-08-25

An implementation that calls back into guest code, such as a sort comparator, declares the
function pointer `extern "sysv64"` and calls it directly; a nested import call from the
callback re-enters the dispatcher as an ordinary call. `qsort` sorts an index permutation and
applies it afterwards, so the guest's array does not move while comparisons run.

**Why:** the compiler then emits the guest's own convention, the same machinery used to enter
a guest, and signal handlers and callback-taking calls reuse it. Leaving the array in place
is stricter than the standard requires and never looser.

**Rejected:**
- A hand-written call shim per callback: duplicates what the compiler emits.
- Sorting the guest's array in place during comparisons: a comparator that inspects neighbours sees elements move.
