# 2026-09-04 - (/loop) The interleaved sequence, and the sixteen cases that were blind

```
differential   314  ->  326 cases
suites 125   tests 2013   clippy/fmt/identity clean
```

Seventeenth cron tick. `strtok_r` had sixteen cases and **not one could tell it from `strtok`**.

## Why they were blind

`strtok_r` differs in exactly one way: the place it keeps is the caller's, not a static. A
sequence that walks one string to its end never exercises that - it is the only client, so a
shared static answers identically. The difference appears only when **two walks overlap**.

## The break, and which cases carried it

Replacing orbistoun's `strtok_r` body with a call to `strtok`:

```text
strtok_r/interleaved#2:        offset is 0x1a, expected 0x2
strtok_r/interleaved#3:        offset is 0x4,  expected 0x2
strtok_r/interleaved-uneven#2: returned 0x1, glibc returned 0x0
```

**Every failure is an interleaved case; not one of the sixteen that existed before.** The same
discipline as D512, where breaking the `va_list` crossover failed exactly the three cases that
crossed.

## No format change, again

The stream index rides as an ordinary `u:` argument. D529 said the format was never the blocker;
this is the second half and needed nothing new either.

One constraint surfaced: `Reference::sequences` groups by the id before `#` **and only merges
consecutive cases**, so an interleaved pair cannot be two sequences named `A` and `B` - it is one
sequence whose steps name their stream. That is the more honest shape anyway, because the
interleaving *is* the case.

The replay keeps a buffer and a saved place per stream, capped at two; a record asking for a
third is **refused rather than folded into stream one**, which would compare the wrong walk while
looking like it worked.

## A placeholder that appeared in the text

The C was written through a substitution using `@` for a newline, because heredocs collapse
backslashes (D530, learned last tick). `@` is also the **poison byte** the buffers are filled
with, so `memset(a, '@', ...)` became `memset(a, '\n', ...)`.

The differential caught it at once: the tokens all agreed and only the padding differed - exactly
the shape that says the function is right and the harness is wrong.

**A placeholder must not be a character that occurs in what it is placed into.**

Decision: [D531](../decisions/D531-the-interleaved-sequence-is-the-only-thing-that-tells-them-apart.md).
