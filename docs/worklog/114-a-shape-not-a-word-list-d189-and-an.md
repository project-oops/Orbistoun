# 2026-08-21 - A shape, not a word list (D189), and an audit that was already red


The `prefix-posix` shape named the whole `scePthreadAttr*` family in one pass - fifteen of
sixteen new names, including `scePthreadExit`, `Yield`, `MutexDestroy` and `MutexTrylock`.
One rule, `sce` + the harvested POSIX name camel-cased, derived rather than written.

**I overstated D168 and have corrected it in place.** D168 said the harvest was missing a
syscall family, and that is *true* - `socket`, `setsockopt`, `sched_yield` are genuinely
absent, because the harvest read only four library directories. It simply was not the reason
the mutexattr family was unreachable; every token that family needs was already harvested.

Worth naming as a failure mode: a **correct** known problem is the most convincing wrong
explanation available. It fits the shape of the question, it is already written down, and
nothing about it looks like a guess.

### The symbol audit fails, and did before this session

`orbistoun-cli audit symbols/generated.json` reports 218 names it cannot account for. Every
one has a derivation record; the records are simply *unverifiable* - they claim
`crates/orbistoun-names/data/standard.txt` and that file does not contain them. Two groups:

- ~187 syscall wrappers absent because the harvest is incomplete. D168's fix - re-harvest
  from a fuller checkout - is the answer, and needs a FreeBSD tree.
- 31 C++ ABI names (`_Unwind_Resume`, `_ZTVN10__cxxabiv1...`, `__cxa_*`) which are not
  FreeBSD at all. They are citable to the Itanium C++ ABI; their record cites the wrong
  source. A vocabulary gap in the *provenance* vocabulary rather than the name one.

Not introduced here - the same 218 fail with or without today's merge, which added 16 names
that all account for themselves. Recorded because the `symbol-provenance` CI job runs this
and would be red, and a check nobody has looked at is worth less than no check.

