# 2026-09-04 - (/loop) The record format already carried what it was said to lack

```
suites 125   tests 2013   clippy/fmt/identity clean
wall unchanged: read of 0x50 at image+0xf56e09, 197 distinct
```

Fifteenth cron tick. No code. The finding is that a blocker quoted for fifteen ticks was never
real.

## What D512 claimed

> What remains needs the format to change first [...] the **wide-character family** needs a wide
> text encoding, and a test that could tell a `strtok_r` delegating to `strtok` needs **two
> interleaved sequences** where the record carries one subject per case.

Both halves are wrong.

## `b:` already carries element data

`Argument::Bytes` is a raw byte blob in hex, and its own documentation says why it exists:

> **Element data cannot ride in a text field**: a four-byte `5` is `05 00 00 00`, and a
> NUL-terminated field would stop at the first of those.

A `wchar_t` string is element data of exactly that shape. **Thirteen cases already use `b:`**,
and the replay marshals it - `qsort` and `bsearch` clone the blob into a mutable array and pass
its address, which is what a wide string needs.

Recording the array is also more honest than a "wide text encoding" would have been: these
functions operate on arrays of `wchar_t`, so a conversion in the record would mean testing the
conversion instead of the function.

## `strtok` is already sequenced

```text
REF|case|strtok/simple#0|strtok|s:a,b,c|s:,
REF|case|strtok/simple#1|strtok|n:|s:,
```

`Reference.cases` is "in the order the run made them", the replay runs them in order, and
`Argument::Null` exists precisely so `strtok(NULL, ..)` is a value - its documentation says
"both occur in the same sequence". Interleaving two sequences is emitting `A#0`, `B#0`, `A#1`.

## What it cost

**`wcslen`, `wcscmp`, `wcsncpy` and `wcsrchr` are declared, implemented, and have zero
differential cases.** They have been verifiable the whole time.

This is D510's pattern in its most expensive form: not a message describing a gap that outlived
the gap, but a **description of a blocker that was never accurate** - repeated into a plan and a
standing prompt for fifteen ticks. Added as check 17: *check a blocker before writing it down,
not after quoting it.*

## And a flake behaved exactly as documented

Two wall-clock tests failed in the whole-workspace run and passed twice alone, then passed in a
second whole-workspace run. That is the documented load-sensitivity; **no assertion was
weakened**, which the plan is explicit about.

Decision: [D529](../decisions/D529-the-record-format-already-carried-what-it-was-said-to-lack.md).
D512 corrected inline.
