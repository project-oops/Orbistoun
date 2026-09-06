# 2026-09-04 - (/loop) `strftime`, and the question that found it

```
differential   326  ->  345 cases
suites 125   tests 2013   clippy/fmt/identity clean
```

Eighteenth cron tick.

## The question, which is the reusable part

Two ticks running had paid off in the same place - **a function orbistoun implements and nothing
has ever compared** - so this asked it mechanically instead of picking a third by hand: which
implemented `libc`/`libScePosix` functions have no differential case?

Twenty. Most are correctly out of scope, and why is worth as much as the answer:

- **Annex K `_s` forms and `strnstr`** - glibc does not provide them. D512 ruled these out and
  was right.
- **`rand`** - would pin orbistoun to *glibc's* generator, which is not the platform's.
- **`strerror`/`strerror_r`** - message text is implementation-specific; the cases would enforce
  glibc's wording as though it were the contract.
- **`memalign`, `strdup`, `strndup`** - answer addresses. `strdup`'s contents are comparable and
  worth doing later; the pointer is not.
- **`strtoll`, `strtoimax`, `strtoumax`** - `strtoimax` delegates to `strtoll`, and on LP64
  `strtol` and `strtoll` clamp identically. The overflow boundary is **already covered**.

That leaves `strftime`.

## Why it was the right one

A **specifier parser** - the shape D511 established as where bugs hide, since both `sprintf` bugs
were in the parser rather than in any conversion. So the format is the case input and the date is
held still, except where a boundary is the point: `%F`/`%T` expansions, `%e` against `%d`, `%y`
at 1999/2000, `%p` at midnight and noon, `%j` being one-based, and the room boundaries.

All nineteen agree. No bug, and a previously uncompared function is now compared.

## What crosses, and what does not

Only the **nine `int` fields** of `struct tm`, as a `b:` blob - ISO C fixes their names and
order, and orbistoun cites FreeBSD's LP64 form where it reads them. Fields past the ninth are
not recorded because orbistoun does not read them, and `%Z`/`%z` are among the conversions it
refuses: a timezone case would test this emulator's lack of a timezone, not its formatting.

**The buffer is compared only when the call succeeded** - ISO C leaves the contents unspecified
otherwise, so comparing them would compare something neither implementation promises.

## The break, on exactly one case

```text
strftime/e-pads-with-space: buffer is 5b30345d..., expected 5b20345d...
```

`[04][04]` where it should be `[ 4][04]` - `0x20` against `0x30`. One byte, one case, **no other
case noticed**.

Decision: [D532](../decisions/D532-strftime-and-the-question-that-found-it.md).
