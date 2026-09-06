# D532 - `strftime`, and the question that found it

**measured** - 2026-09-04 (nineteen cases against glibc 2.39)

```text
differential   326  ->  345 cases
```

## The question, which is the reusable part

Two ticks running had found value in the same place: **a function orbistoun implements and
nothing has ever compared.** So rather than pick a third by hand, ask it mechanically - which
implemented `libc`/`libScePosix` functions have no differential case?

Twenty. Most are correctly out of scope, and saying why is worth as much as the answer:

- **The Annex K `_s` forms** (`memcpy_s`, `strcpy_s`, `strncpy_s`, `wcsncpy_s`, and five more)
  and **`strnstr`** - glibc does not provide them, so there is nothing to compare against. D512
  already ruled these out and it was right.
- **`rand`** - comparing it would pin orbistoun to *glibc's* generator, which is not the
  platform's. A differential can only compare what both sides are supposed to agree on.
- **`strerror` / `strerror_r`** - the message text is implementation-specific. Same trap: the
  cases would enforce glibc's wording as though it were the contract.
- **`memalign`, `strdup`, `strndup`** - answer addresses. `strdup`'s *contents* are comparable
  and it is worth doing later; the pointer is not.
- **`strtoll`, `strtoimax`, `strtoumax`** - `strtoimax` delegates to `strtoll`, and on an LP64
  target `strtol` and `strtoll` clamp identically, so cases here would verify a delegation. The
  overflow boundary they would test is **already covered** for `strtoul`/`strtoull`/`strtol`.

That leaves `strftime`, which is the one worth having.

## Why `strftime` was the right one

It is a **specifier parser**, which is the shape D511 established as where bugs hide: two
`sprintf` bugs lived in one, and neither was in any conversion - both were in the parser around
them. So the format is the case input and the date is held still, except where a boundary is the
point.

All nineteen agree. No bug, and a previously uncompared function is now compared.

## What crosses, and what deliberately does not

Only the **nine `int` fields** of `struct tm`, as a `b:` blob. ISO C fixes their names and order,
both sides agree on them, and orbistoun cites FreeBSD's LP64 form where it reads them. The
fields past the ninth are not recorded because orbistoun does not read them - and the
conversions that would need them, `%Z` and `%z`, are among the ones it refuses. Recording a
timezone case would test this emulator's lack of a timezone rather than its formatting.

**The buffer is compared only when the call succeeded.** ISO C leaves the contents unspecified
when the result does not fit, so the reference records none there and neither does the replay.
Comparing them would be comparing something neither implementation promises.

## The break, on exactly one case

`%e` is space-padded where `%d` is zero-padded. Breaking it to zero-pad:

```text
strftime/e-pads-with-space: buffer is 5b30345d5b30345d..., expected 5b20345d5b30345d...
```

`[04][04]` where it should be `[ 4][04]` - `0x20` against `0x30`, one byte, one case. **No other
case noticed**, which is what a case written for a specific difference should look like when
that difference appears.

## What these cannot prove

That the console's `struct tm` is laid out this way. Both sides assume the ISO C order; if the
target differs, every case agrees and both are wrong together. That is the standing limit of a
differential - it compares two implementations, and neither against hardware - and it is written
into the test rather than left to be inferred.
