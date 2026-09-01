# 2026-08-22 - FreeBSD harvest: the walker ignored its own rule (D191)


Cloned FreeBSD sparse and blobless, re-harvested, and the word list **shrank** - 147 names
lost, six gained. Chasing that found a defect older than the exercise.

`is_version_script` accepts any `.map` file, and its doc comment records why: `libthr` calls
its exports `pthread.map`, and a rule looking for `Symbol.map` cost every `pthread_*` name
(D127). **The harvest command never called it** - it tested the filename directly, and had
since before D127 was written. The fix landed in the crate and in an example binary; the
command anybody actually runs kept the original rule and kept reporting success.

Ten files of fifty-seven were being skipped. Wired up, plus `lib/libsys` added to the
directory list (FreeBSD moved syscall stubs out of `libc`): **2685 -> 3064 names.**

A test caught the consequence exactly as designed. It asserted the *absence* of
`clock_gettime` with the message "if this now passes, syscall stubs became harvestable and
the note above is stale" - and the note claimed, as a fact about the world, that FreeBSD
generates syscall stubs at build time so no version script declares them. False:
`lib/libsys/Symbol.sys.map` declares every one. An inference from a search that missed,
written down as a property of the source. The assertion is kept and inverted.

### Two predictions, both wrong, both from the head of a list

I recommended this as the highest-value action on two grounds and neither held.

- "It will turn ~187 provenance records green." It turned a handful green. The 219
  unaccounted are 202 vendor `sce*` names and 17 C++ ABI names - **not** syscall wrappers.
  I read five names off the top of the audit output and generalised to the whole list.
- "It will plausibly name several of the corpus's unnamed imports." It names **none** of
  the twenty-six.

Worth doing anyway, for a reason neither prediction named: it surfaced a defect that had
been silently degrading every harvest since D127. But the estimates were confident and
wrong, from exactly the sampling error that has run through this whole session.

