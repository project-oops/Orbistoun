# D529 - The record format already carried what D512 said it lacked

**decided** - 2026-09-04

D512 closed with a forward-looking claim that has been quoted as a blocker in every plan since:

> Two hundred and ninety-five cases, and every libc function that can be driven from the record
> format as it stands now has some. What remains needs the format to change first, and both
> items are the same shape: the **wide-character family** needs a wide text encoding, and a test
> that could tell a `strtok_r` delegating to `strtok` needs **two interleaved sequences** where
> the record carries one subject per case.

**Both halves are wrong.** The format carries both today, and has since before D512 was written.

## The wide family needs `b:`, which exists

`Argument::Bytes` - the `b:` field - is a raw byte blob in hex, and its own documentation says
why it exists:

> **Element data cannot ride in a text field**: a four-byte `5` is `05 00 00 00`, and a
> NUL-terminated field would stop at the first of those. Its own type, no escaping to get wrong.

A `wchar_t` string *is* element data of exactly that shape. Thirteen cases already use `b:`, and
the replay already marshals it - `qsort` and `bsearch` clone the blob into a mutable array and
pass its address, which is the same thing a wide string needs.

Encoding the array directly is also the more honest choice than a "wide text encoding" would
have been: the functions under test operate on arrays of `wchar_t`, so recording the array
avoids inventing a conversion and then testing the conversion instead of the function.

## The interleaved sequence needs ordering, which exists

`strtok` is **already sequenced** in the corpus:

```text
REF|case|strtok/simple#0|strtok|s:a,b,c|s:,
REF|case|strtok/simple#1|strtok|n:|s:,
REF|case|strtok/simple#2|strtok|n:|s:,
```

`Reference.cases` is "the cases, in the order the run made them", the replay runs them in that
order, and `Argument::Null` exists precisely so `strtok(NULL, ..)` is a value rather than a
missing argument - its documentation says "both occur in the same sequence".

So interleaving two sequences is emitting `A#0`, `B#0`, `A#1` as three ordered cases. Nothing
about "one subject per case" prevents it; a case is one *call*, and a sequence is what the order
of cases already means. For `strtok_r`, which save pointer a call uses is an ordinary argument
and needs no new type.

## What this costs, and why it is worth a decision rather than a fix

Four functions - `wcslen`, `wcscmp`, `wcsncpy`, `wcsrchr` - are **declared and implemented in
orbistoun and have zero differential cases**. They have been verifiable the whole time.

The cost was not the format. It was that a decision's forward-looking sentence, written without
checking the format it was describing, became the reason nobody looked. This is D510's pattern
in its most expensive form yet: not a message describing a gap that outlived the gap, but a
**description of a blocker that was never accurate**, repeated into a plan and a standing prompt
for fifteen ticks.

D510 said the habit is to grep for a gap's own words when a change closes one. This is the other
half: **grep for them when the gap is first written down**, because the cheapest moment to find
out a blocker is imaginary is before it is quoted.

## What is not decided here

How the wide cases should be *shaped* - which inputs are worth recording. That is the same
question every other family faced and D511 answered generally: vary **the parameter nobody
varied**. For the wide family the obvious candidates are element width assumptions, embedded
values above the ASCII range, and a `wcsncpy` that does or does not terminate - none of which
needs a format change either.
