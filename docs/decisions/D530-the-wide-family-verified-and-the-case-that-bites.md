# D530 - The wide family is verified, and one case is the reason it was worth doing

**measured** - 2026-09-04 (nineteen cases against glibc 2.39)

D529 established that the record format already carried wide strings. This writes the cases.

```text
differential   295  ->  314 cases
```

Four functions - `wcslen`, `wcscmp`, `wcsncpy`, `wcsrchr` - were **declared, implemented and
entirely unverified**. All nineteen cases agree.

## The parameter nobody varied is the element width

Every narrow-family case in this corpus reads bytes, so nothing in it could ever have caught an
implementation that walks bytes where it should walk elements. That is the case worth building:

```text
wcslen/narrow-would-stop-early   b:41000000 00010000 42000000 00000000   ->  3
```

`L'A'`, then `U+0100`, then `L'B'`. The second element's **low byte is zero**, so an
implementation that walks bytes stops at one where the answer is three. Breaking `wcslen` to
step one byte instead of four:

```text
wcslen/narrow-would-stop-early: returned 0x1, glibc 2.39 returned 0x3
wcslen/ascii:                   returned 0x9, glibc 2.39 returned 0x3
wcslen/above-ascii:             returned 0x1, glibc 2.39 returned 0x3
```

The designed case fires, and so do two others - which is the check that the case is doing
something the ordinary ones do not, rather than being one more of them.

The same idea, one function along: `wcscmp/differs-above-the-low-byte` compares `0x41` against
`0x141`. **The low bytes are identical**, so a byte-wise comparison calls them equal; the
recorded sign is negative.

## Bytes in, elements out

The subject rides as a `b:` blob because it is element data. But `wcsrchr` answers a pointer
*into* that array, and the reference records `at - s` in `wchar_t` units, because that is what
pointer arithmetic on a `wchar_t *` produces. The replay divides by four for the same reason -
that is not a conversion, it is the same arithmetic on the other side.

## What these cannot prove, and it is in the test

**That a wide character is four bytes on the console.** Both sides assume it: glibc by its own
definition, orbistoun by saying so where it implements them. If the target's `wchar_t` is not
four bytes, every case here agrees and both are wrong together - which no differential can
catch, because a differential compares two implementations and neither against hardware.

orbistoun already records that as an assumption on `wcslen` ("a 16-bit `wchar_t` would make this
count double and nothing in a trace would say so"). Nineteen agreeing cases do not retire it.

## And the heredoc hazard is real

The loop file has said "bash heredocs collapse backslashes" for weeks. Writing these helpers
through one turned every `\n` in a `printf` into a real newline, and the reference did not
compile. Repairing it then hit the second half of the same trap: `re.sub`'s *replacement* string
also processes backslashes, so the first repair reinserted the newlines it was removing. A
lambda replacement fixed it.

Both are instrument notes, not findings - but the first one is written down and I walked into it
anyway, which is worth more than the note.
