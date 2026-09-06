# 2026-09-02 - (/loop) Bulk port batch 1: the bounded string and memory functions

First batch under D472. Eleven functions written from their specifications and tested against
them - **no guest run involved at any point**, which is the whole claim the bulk approach rests on.

The measurement is the gap falling, not a title getting further:

```
documented   711 needed, 452 missing   ->   711 needed, 441 missing
```

## What went in

All in `orbistoun-libc/src/cstring.rs`, each carrying its specification in a comment:

- **BSD**: `strlcpy` (FreeBSD `strlcpy(3)`), `strnstr` (FreeBSD `strnstr(3)`).
- **C95 wide**: `wcscmp` (7.29.4.4.1), `wcsncpy` (7.29.4.2.2).
- **C11 Annex K**: `memcpy_s`, `memmove_s`, `memset_s`, `strcat_s`, `strncat_s`, `strncpy_s`,
  `wcsncpy_s` (K.3.7.x, K.3.9.2.1.1).

The three details worth having written down, because each is a specification saying the opposite
of what the name suggests:

- **`strlcpy` answers the length of the *source*, not of the copy.** That is precisely what lets a
  caller detect truncation; answering the copied length looks right in every fitting case and is
  wrong exactly when it matters.
- **`memset_s` writes even when it refuses.** It is the function you scrub a secret with, so the
  standard requires the store on the constraint-violation path too. An early return would leave
  the secret in place and report an error nobody reads.
- **`wcsncpy_s` counts wide characters, not bytes.** Counting bytes would let four times too much
  through a *bounds-checking* function.

Eleven tests, and the first of each pair is the guard made to fail: `memcpy_s` refusing an
oversized copy **and scrubbing** the destination, `strcat_s` refusing an append whose terminator
would not fit, `strnstr` declining a match that starts inside its window but runs past it,
`wcsncpy_s` refusing two characters plus a terminator in a field of two. The fitting cases are
tested beside them so the guards are known to be discriminating rather than always failing.

## What was deliberately left out

- **`wcstombs` / `wcsrtombs`** - they need a charmap, and which bytes the C locale admits above
  ASCII is not something to guess at. They go in a locale batch where the question can be
  answered rather than assumed.
- **`wcsmisc`** - appears in the import list and is not a standard function name. Nothing here
  will invent one; it is a Dinkumware internal to be identified before it is written.
- `strtoumax` - trivially `strtoull` at this width, but it belongs with the conversion family in
  a later batch rather than half-done here.

## Method note

`RSIZE_MAX` is implementation-defined, so the Annex K functions deliberately do **not** assert a
ceiling on it - the constraint they exist for is `n <= s1max`, and inventing a limit to look
thorough would be inventing a constant (principle 3).

clippy `--tests` clean, fmt clean, orbistoun-libc tests pass, nothing committed.

**Next batch**: the Dinkumware C runtime internals - the `_Atomic_*` family, `_Thrd_*`, the file
and system locks, `_Stoul`/`_Stoull`, the `_F*` float variants, and `_Stdout`/`_Stderr`, which are
**data objects rather than functions** and need the `ImportKind::Object` path (D307) rather than a
thunk.
