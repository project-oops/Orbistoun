# 2026-09-02 - (/loop) More differential cases, and the parallel run finds a real concurrency bug

```
differential cases    24  ->  63   (63 rebuilt, 63 agreed, 0 diverging)
tests               1944  -> 1947
```

Thirty-nine new cases across the `str*`/`mem*` family, the bounded copies and `strtod`.
**Orbistoun agreed on every one.** That is worth stating plainly after yesterday's four bugs:
the conversion and formatting corners were wrong, and the string and search family is not.

## What the new cases cover

- **Comparisons** - `strcmp`, `strcasecmp`, `strncmp`, `strncasecmp`, `memcmp`, including the
  prefix edge, both empty strings, and a zero bound.
- **Searches** - `strchr`, `strrchr`, `memchr`, `strstr`, including finding the terminator,
  an empty needle, a needle past a `memchr` length, and a zero length.
- **Bounded copies** - `strncpy` and `strncat` into a poisoned window, which is the only way
  to see that `strncpy` pads the whole remainder with NULs when the source is short and does
  **not** terminate at all when it is not.
- **`strtod`** - compared as an exact bit pattern, because a decimal rendering hides precisely
  the last-place differences worth catching.

## Three things the cases forced the harness to grow

**A comparison's contract is the sign, not the value.** ISO C says greater than, equal to or
less than zero and no more. glibc returns the byte difference; another implementation may
return exactly the sign. Asserting the magnitude would report a *conforming* difference as a
bug, so the reference records no return value for these at all and the sign is a field of its
own. A differential that manufactures false alarms gets ignored, which is worse than not
having one.

**A byte is not a character.** `strcmp` compares as `unsigned char` by definition, so `"\x80"`
against `"a"` is positive and an implementation using a signed char answers backwards - a real
bug class worth a case. Writing that byte raw made the record file stop being text. The format
now escapes anything that would not survive it - high bytes, the pipe, the backslash - and the
argument type is `Vec<u8>` rather than `String`, because a `String` could not hold the case at
all.

**`strtod` answers in a floating-point register**, so `implementation_named` could not find it.
`float_implementation_named` is the other door; a function answers in `rax` or in `xmm0` and
never both (D268), so a harness looking one up by name has to say which it wants.

## The assertion that was too weak, and what it was hiding

The agreement check ended with `assert!(agreed > 0)`. That is the shape that lets a
differential rot into nothing: a checker that silently stopped comparing sixty of sixty-three
cases would still pass it. **Counting successes is not checking for failures.**

It now asserts `agreed + diverging == rebuilt`, and that was verified by making it fail. The
answer is 63, 63, 0.

## And then the workspace run found a concurrency bug

Two condition-variable tests failed during `cargo test --workspace` and passed every time the
suite was run alone. That is what a spurious wakeup looks like, and it was one:

**`cond_wait` treated any wake as a signal.** `Condvar::wait` may return without anybody having
signalled, and the code returned `true` for whatever came back. So an untimed waiter reported
it had been signalled before anything signalled, and a timed wait on a condition variable
nothing ever touched reported success.

The count is the condition; a wake is only a prompt to re-read it. It now loops, with the
deadline re-read each turn so a run of spurious wakes cannot extend the total wait - **the
same discipline `wait_while` already applies to every other primitive in the module**, which
is where it should have been taken from in the first place.

### Reproduced deterministically, because a spurious wake cannot be summoned

`cond_broadcast` owes one wake and calls the host's `notify_all`, so *both* waiters are woken
and the count is what says only one was signalled. Under the old code both returned "signalled":
two guest threads leaving a condition only one of them was told about. The new test asserts
exactly one is released, and it was watched to fail - reintroducing the single-wake behaviour
gives `[Some(true), Some(true)]`.

**The parallel run is what surfaced this**, which is the same lesson as the red gates: a
narrower check that passes is not the same as a broader one that would.

## State

`cargo test --workspace` green - **117 suites, 1947 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. `also differential` reports `63 cases, matching
tools/differential/reference.c`.

Nothing committed. 20:13 UK, and the day holds worklogs 292-321 and D466-D479.

**Next**: `qsort` and `bsearch`, which are the interesting ones left - both call *back* into
guest code, so the reference and orbistoun have to agree about a comparison callback rather
than only about a return value. Then `strtok`, whose answer depends on state carried between
calls, which needs a sequence rather than a single case. After that the sign-extended return
width, which wants a decision before a sweep.
