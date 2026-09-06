# 2026-09-04 - (/loop) The wide family verified, and the case that bites

```
differential   295  ->  314 cases   (19 wide, all agreeing)
suites 125   tests 2013   clippy/fmt/identity clean
```

Sixteenth cron tick. D529 said the format already carried wide strings; this writes the cases.

`wcslen`, `wcscmp`, `wcsncpy`, `wcsrchr` were **declared, implemented and entirely unverified**.
All nineteen cases agree.

## The parameter nobody varied is the element width

Every narrow case in the corpus reads bytes, so none of them could ever catch an implementation
that walks bytes where it should walk elements:

```text
wcslen/narrow-would-stop-early   b:41000000 00010000 42000000 00000000   ->  3
```

`L'A'`, `U+0100`, `L'B'` - the second element's **low byte is zero**. Breaking `wcslen` to step
one byte instead of four:

```text
wcslen/narrow-would-stop-early: returned 0x1, glibc 2.39 returned 0x3
wcslen/ascii:                   returned 0x9, glibc 2.39 returned 0x3
wcslen/above-ascii:             returned 0x1, glibc 2.39 returned 0x3
```

The designed case fires **and so do two others**, which is the check that it does something the
ordinary cases do not rather than being one more of them.

One function along: `wcscmp/differs-above-the-low-byte` compares `0x41` with `0x141` - identical
low bytes, so a byte-wise comparison calls them equal. Recorded sign: negative.

## Bytes in, elements out

The subject rides as `b:` because it is element data; but `wcsrchr` answers a pointer into that
array and the reference records `at - s` in `wchar_t` units, because that is what pointer
arithmetic on a `wchar_t *` produces. The replay divides by four for the same reason - not a
conversion, the same arithmetic on the other side.

## What these cannot prove, said in the test

**That a wide character is four bytes on the console.** Both sides assume it. If the target's
`wchar_t` is not four bytes, every case agrees and both are wrong together - which no
differential can catch. orbistoun already records that as an assumption on `wcslen`; nineteen
agreeing cases do not retire it.

## The heredoc hazard is real, and written down, and I walked into it

Writing the C helpers through a bash heredoc turned every `\n` in a `printf` into a real newline
and the reference would not compile. The repair then hit the second half of the same trap:
`re.sub`'s *replacement* also processes backslashes, so the first fix reinserted the newlines it
was removing. A lambda replacement fixed it.

Decision: [D530](../decisions/D530-the-wide-family-verified-and-the-case-that-bites.md).
