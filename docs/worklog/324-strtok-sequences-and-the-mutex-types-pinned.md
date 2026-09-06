# 2026-09-02 - (/loop) `strtok` needs a sequence, and the mutex type mapping becomes a test

```
differential cases    76  ->  96   (96 rebuilt, 96 agreeing, 0 diverging)
hardware claims        7  ->  11   (16 outstanding)
tests               1951  -> 1954
```

## `strtok` cannot be one case, twice over

**Its answer depends on the previous call.** The first takes the subject and each later one
passes null to mean "carry on", so a single case only ever exercises the first token. The
record numbers the steps - `strtok/simple#0`, `#1`, `#2` - and the checker groups by the name
before the `#` and replays them in order against one buffer.

**And it mutates what it walks.** `strtok` writes a NUL over each delimiter it consumes, so
half the contract is invisible in the return values: a shim answering the right tokens without
writing the terminators is wrong in a way only the bytes show. The buffer is recorded after the
last step and compared.

`a,,b` proves both halves at once - two tokens, not three, because C skips runs of delimiters,
and the buffer comes back `61 00 2c 62 00`, so the *first* comma became a terminator and the
second was left alone.

Twenty steps across seven sequences: empty fields, a leading delimiter, a trailing one, no
delimiter present, all delimiters, two delimiter characters, and one call past the end. All
agree.

Two things the format needed: **a null argument type**, because `strtok(NULL, ..)` and
`strtok("", ..)` are different calls and both occur here; and the sequence grouping, where a
case with no `#` is simply a sequence of one so everything walks the same way.

### The lock, which is the function being honest rather than the test

Orbistoun keeps `strtok`'s place in a process-wide value. That is **correct** - ISO C says
`strtok` is not reentrant - but this file's tests run on several threads, so two sequences
interleaved would each see the other's position. The runner takes a mutex, and says in the code
that it is a property of the function rather than a workaround.

## The mutex type mapping stops being a comment

`mutex_recursion_from_attr` carries the platform's type numbering - `2` is recursive, `4` is
error-checking, anything else a plain lock - with a comment citing `015-sync/mutex-recursion`
as where it came from. **A comment citing a measurement is not checked by anything.**

It is now four assertions: set each type on an attribute object, initialise a lock with it, take
it twice, and compare what the second `Trylock` answers against what the console answered.
`0x80020010` for types 1 and 3, `0x0` for the recursive 2, `0x80020016` for the error-checking
4. All four agree, so changing the mapping now makes four measurements disagree with the machine
they were derived from.

Compared at **thirty-two bits**, per D480 - the records read `0xffffffff8002_xxxx` because the
probe widened a C `int`, and the rest of the register was never observed.

## A gate of my own that lied

Running the checks as `python -c "..." && cargo clippy ... ; echo "CLIPPY CLEAN"` printed
**CLIPPY CLEAN when clippy had not run at all** - the Python failed, `&&` short-circuited, and
the `;` after it happily printed the reassuring line anyway.

Exactly the failure this project keeps writing down, in a shell one-liner: *a number - or a
word - a gate prints is a claim like any other.* Re-run separately, the warning was still
there.

## State

`cargo test --workspace` green - **117 suites, 1954 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. `also hardware`: 38 measurements, 27 constant, 11
claimed and 16 outstanding. `also differential`: 96 cases matching the reference program.

Nothing committed. The day holds worklogs 292-324 and D466-D480.

**Next**: of the sixteen outstanding, the cheapest left are the two `sceKernelDirectMemoryQuery`
flag cases, which need a direct-memory allocation to query rather than a single call. After
those the loader ones need the TitleOwn loader, which is PPSA02664's real wall. On the
differential side, `memmove` overlap and the `_s` bounded forms - checking first what glibc
actually provides, since Annex K is optional and largely absent there.
