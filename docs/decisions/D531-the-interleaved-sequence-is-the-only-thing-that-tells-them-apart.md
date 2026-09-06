# D531 - The interleaved sequence is the only thing that tells `strtok_r` from `strtok`

**measured** - 2026-09-04 (twelve cases, and a break that fires on exactly those twelve)

```text
differential   314  ->  326 cases
```

`strtok_r` had sixteen cases before this and **not one of them could tell it from `strtok`**.

## Why every existing case was blind to it

`strtok_r` differs from `strtok` in exactly one way: the place it keeps between calls is the
caller's, not a static. A sequence that walks one string to its end never exercises that - the
only client is the sequence itself, so a shared static answers identically.

The difference appears only when **two walks overlap**: start A, start B, continue A. An
implementation that ignores the save pointer loses A's place when B starts.

## The break, and which cases carried it

Replacing orbistoun's `strtok_r` body with a call to `strtok`:

```text
strtok_r/interleaved#2:        offset is 0x1a, expected 0x2
strtok_r/interleaved#3:        offset is 0x4,  expected 0x2
strtok_r/interleaved#4:        returned 0x0, glibc returned 0x1
strtok_r/interleaved-uneven#2: returned 0x1, glibc returned 0x0
```

**Every failure is an interleaved case, and not one of the sixteen that existed before.** That
is the whole claim, demonstrated rather than argued - the same discipline as D512, where
breaking the `va_list` crossover failed exactly the three cases that crossed and none of the
six that did not.

## No format change, again

The stream index rides as an ordinary `u:` argument. D529 established the format was never the
blocker; this is the second half of that, and it needed nothing new either.

One constraint did show up and is worth recording: `Reference::sequences` groups by the id
before `#` **and only merges consecutive cases**, so an interleaved pair cannot be two sequences
named `A` and `B`. It is one sequence whose steps name their stream - which is the more honest
shape anyway, because the interleaving *is* the case.

The replay keeps a buffer and a saved place per stream, capped at two. A record asking for a
third is **refused rather than folded into stream one**, which would compare the wrong walk
while looking like it worked.

## And a placeholder that appeared in the text

The C helper was written through a substitution using `@` to stand for a newline, because a bash
heredoc collapses backslashes (D530, learned last tick). `@` is also the **poison byte** the
buffers are filled with, so the substitution rewrote `memset(a, '@', ...)` into `memset(a, '\n',
...)` and the buffers came back poisoned with `0a`.

The differential caught it immediately - the tokens all agreed and only the padding differed,
which is exactly the shape that says "the function is right and the harness is wrong". Fixed by
restoring the poison and regenerating.

**A placeholder must not be a character that occurs in what it is placed into.** Cheap to say
afterwards; it cost a regeneration cycle.
