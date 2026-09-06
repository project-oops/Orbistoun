# D535 - `strdup` covered, and a missing terminator is only caught by luck

**measured** - 2026-09-04 (eight cases, and a break run three times)

```text
differential   400  ->  408 cases
```

`strdup` and `strndup` were the last genuinely differentiable pair from D532's census. They are
covered now, and the more useful half of this is what the break said about *how well*.

## The pointer is not compared; the contents are

An address is a fact about one process - the same reasoning the search family already follows,
one step on. What crosses is found-or-not, then the copy **through its terminator**, because the
terminator is the half of `strndup` that is easy to lose.

The discriminating case has to have a source **longer** than the bound. A `strncpy`-shaped
implementation is correct whenever the source fits, so every case where it does passes for a
broken one.

## And then the break did not behave

Removing the terminator and running three times:

```text
run 1   strdup/embedded-high-byte  strndup/bound-longer-than-source  strndup/truncates
run 2   strdup/embedded-high-byte  strdup/plain  strndup/bound-longer-than-source
run 3   strdup/embedded-high-byte  strdup/plain  strndup/bound-longer-than-source  strndup/truncates
```

**Three, three and four - and different sets.** The byte after an unterminated copy is whatever
the allocator last left there, and it is a zero often enough that the read finds a terminator
that was never written.

## So the claim had to be narrowed

The doc comment said the `unterminated` marker made this deterministic. It does not. What these
cases actually verify is the **contents of a copy that is terminated**; termination itself is
caught *probabilistically*.

That distinction is the whole value of running the break more than once. A single run showed
four failures and would have been written up as "the cases catch it" - which is the confident
wrong answer this file exists to avoid, in a decision about a test.

Catching it properly needs the allocation poisoned before the call. That is the reference's
technique for buffers it owns (D511's `snprintf` window, `strncpy`'s padding) and it is **not
available to a caller of `strdup`**, which does not choose the memory. Recorded as a limit
rather than worked around.

## The rule this sharpens

"A guard is not finished until somebody has made it fail" has an unstated second half:
**a break that fires is not the same as a break that fires reliably.** Where the failure depends
on memory nobody wrote, run it more than once - the same reason three samples is the floor for a
run, applied to a test.
