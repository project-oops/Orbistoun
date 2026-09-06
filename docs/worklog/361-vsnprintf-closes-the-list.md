# 2026-09-03 - (/loop) `vsnprintf` closes the list, and the break names the cases carrying it

```
differential   286  ->  295 cases
```

Arrived by hand again - the eighteenth wakeup that did not fire.

## The last uncovered function needed a `va_list` built rather than borrowed

`vsnprintf` was last because its arguments do not arrive as arguments. The System V psABI puts
the first six integer arguments in a register save area and the rest on the stack, and orbistoun
reads a guest's four-field `va_list` exactly as that defines it.

Rust cannot hand over a C `va_list`, and it does not need to: **the layout is published, and
what is compared is what was rendered** - not how the arguments were arranged to get there. So
the test builds the simplest valid list, `gp_offset` at zero with six slots and an overflow
area, which is what a caller with seven or more integer arguments produces.

## The sweep is over argument *count*, and breaking it proved why

Every earlier formatting case had one argument, so none ever left the save area. These go one,
two, six, seven, eight. Breaking the crossover:

```text
vsnprintf/seven-crosses-into-the-stack   FAILED
vsnprintf/eight-arguments                FAILED
vsnprintf/mixed-across-the-crossover     FAILED
```

**Exactly three, and not one of the six-or-fewer cases.** That is both halves in one run: the
crossover is genuinely exercised, and the smaller cases could never have caught it. An
implementation reading only the save area renders six correctly and the seventh from whatever
follows.

Same lesson as D511 one level up - there the parameter nobody varied was the specifier, here it
is the arity. **Every bug this differential has found came from making a parameter an input.**

## What it cannot check, written in the function rather than assumed

The reference's list has three named parameters before its variadic ones, so its `gp_offset`
starts partway up the save area where the constructed one starts at zero. Both are valid and
must render the same text - but a bug appearing only at a non-zero starting offset would not be
caught. Said so beside the construction.

## Where the differential stands

**295 cases**, and every libc function drivable from the record format as it stands now has
some. What remains needs the format changed first, and both items are the same shape: the
wide-character family needs a **wide text encoding**, and telling a `strtok_r` that delegates to
`strtok` needs **two interleaved sequences** where the record carries one subject per case.
D512 records that as a decision to take deliberately rather than while adding a function.

## State

`cargo test --workspace` green - 119 suites, **1998 tests**, 0 failures. Differential **295
cases**, all agreeing. clippy `--tests` clean, fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-361 and D466-D512.

**Next**: the record-format decision, which is now the only thing standing between the
differential and the wide-character family.
