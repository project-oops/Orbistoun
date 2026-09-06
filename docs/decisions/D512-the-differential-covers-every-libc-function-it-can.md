# D512 - `vsnprintf` closes the list, and the break proves which cases were carrying it

**measured** - 2026-09-03 (nine cases through a constructed `va_list`)

`vsnprintf` was the last differential-eligible function with no coverage, and it was last
because it is the only one whose arguments do not arrive as arguments: it takes a `va_list`,
and on this architecture that is not a pointer walking a stack.

## The list is built, not borrowed

The System V psABI defines four fields - `gp_offset`, `fp_offset`, `overflow_arg_area`,
`reg_save_area` - and the first six integer arguments live in the save area while everything
past them sits on the stack. Rust cannot hand over a C `va_list`, and it does not need to: the
layout is published, and **what is compared is what was rendered, not how the arguments were
arranged to get there**.

So the test builds the simplest valid list - `gp_offset` at zero, six slots in the save area,
the rest in the overflow area. That is exactly what a caller with seven or more integer
arguments produces.

## The sweep is over the *number* of arguments, and the break shows why

Every earlier formatting case had one argument, so none of them ever left the register save
area. The cases here go one, two, six, seven, eight - and breaking the crossover makes the
point better than the argument does:

```text
vsnprintf/seven-crosses-into-the-stack   FAILED
vsnprintf/eight-arguments                FAILED
vsnprintf/mixed-across-the-crossover     FAILED
```

**Exactly three, and not one of the six-or-fewer cases.** An implementation that only ever reads
the save area renders the first six correctly and the seventh from whatever follows it, and
every case below seven passes over that in silence. The break demonstrates both halves: the
crossover is genuinely exercised, and the smaller cases could never have caught it.

This is the same lesson as D511 one level up. There the parameter nobody varied was the
*specifier*; here it is the *arity*. Every bug this differential has found came from making a
parameter an input rather than writing more cases at one setting.

## What these cases cannot check, said here rather than assumed

The reference's own list has three named parameters before its variadic ones, so its `gp_offset`
starts partway up the save area where the constructed one starts at zero. Both are valid and
both must render the same text - that is the property under test - but **a bug that appears only
at a non-zero starting offset would not be caught**. Written into the function's own
documentation, beside the construction it describes.

## Where the differential stands

Two hundred and ninety-five cases, and every libc function that can be driven from the record
format as it stands now has some. What remains needs the format to change first, and both items
are the same shape: the **wide-character family** needs a wide text encoding, and a test that
could tell a `strtok_r` delegating to `strtok` needs **two interleaved sequences** where the
record carries one subject per case. Those are a design decision, not more cases, and are worth
taking deliberately rather than while adding the next function.


> **Both halves of that paragraph are wrong - see D529.** The format already carries both. `b:`
> is a raw byte blob whose own documentation gives a four-byte element as its motivating case,
> which is exactly what a `wchar_t` string is; thirteen cases use it and the replay already
> marshals it. And `strtok` is *already* sequenced as ordered cases (`#0`, `#1`, `#2`) with `n:`
> for the NULL continuation, so interleaving two sequences is emitting `A#0`, `B#0`, `A#1`.
>
> Nothing was blocked. `wcslen`, `wcscmp`, `wcsncpy` and `wcsrchr` are implemented, have no
> cases, and have been verifiable the whole time. This paragraph was written without checking
> the format it describes, and was then quoted as a blocker for fifteen ticks.
