# 2026-09-04 - (/loop) `strdup` covered, and a missing terminator is only caught by luck

```
differential   400  ->  408 cases
suites 125   tests 2013   clippy/fmt/identity clean
```

Twenty-first cron tick. `strdup`/`strndup` were the last genuinely differentiable pair from the
census. The more useful half of this tick is what the break said about *how well* they are
covered.

## The pointer is not compared; the contents are

An address is a fact about one process. What crosses is found-or-not, then the copy **through
its terminator** - the half of `strndup` that is easy to lose. The discriminating case needs a
source **longer** than the bound, because a `strncpy`-shaped implementation is correct whenever
the source fits.

## Then the break did not behave

Removing the terminator and running three times:

```text
run 1   strdup/embedded-high-byte  strndup/bound-longer-than-source  strndup/truncates
run 2   strdup/embedded-high-byte  strdup/plain  strndup/bound-longer-than-source
run 3   strdup/embedded-high-byte  strdup/plain  strndup/bound-longer-than-source  strndup/truncates
```

**Three, three and four - different sets.** The byte after an unterminated copy is whatever the
allocator last left there, and it is a zero often enough that the read finds a terminator nobody
wrote.

## So the claim was narrowed

The doc comment claimed the `unterminated` marker made this deterministic. It does not. These
cases verify the **contents of a copy that is terminated**; termination itself is caught
*probabilistically*, and the comment now says so.

A single run showed four failures and would have been written up as "the cases catch it" - the
confident wrong answer, inside a decision about a test.

Catching it properly needs the allocation poisoned before the call, which is the reference's
technique for buffers it owns and **is not available to a caller of `strdup`**. Recorded as a
limit rather than worked around.

## The rule this sharpens

"A guard is not finished until somebody has made it fail" has an unstated second half: **a break
that fires is not the same as a break that fires reliably.** Where the failure depends on memory
nobody wrote, run it more than once.

Decision: [D535](../decisions/D535-a-missing-terminator-is-only-caught-by-luck.md).
