# D599 - The dispatcher was measuring a different program

**Status:** measured
**Date:** 2026-09-08

## Two commands, one title, two walls

```text
./bin/orbistoun run PPSA02664       fault image+0x39f7c,   read of 0xffffffffffffffff
orbistoun-cli run <the same file>   fault image+0xf56e09,  read of 0x66035
```

The driver passes `--symbols-db`. The bare command does not, and **`orbistoun-cli turn` never
did**.

The database decides which imports have names; a named import is one an implementation can be
found for, and an unnamed one lands on a stub. So a run without it is not a noisier version of
the same program - it is a program in which a different set of functions is unimplemented.
Confirmed by handing the bare command the database and watching the wall move back:

```text
orbistoun-cli --symbols-db symbols/generated.json run …   fault image+0x39f7c
```

## What that cost

**Every dispatcher verdict this session was taken against the wrong configuration** and then
compared against runs from the loop, which uses the right one. `Step::CheckRepeats` reported *two
runs agree, fault 0x66036* - true of the program it ran, and about a wall the loop never sees.

It also produced the mistake corrected in D598: instruction bytes read out of a turn transcript
were treated as the baseline. They were from a run that was both under a sweep *and* under the
wrong symbol set, and the value `0x8a6c003d` was written into a decision entry as the code the
guest wanted at its wall. It is not; it is real guest code at a site the baseline never reaches.

## Fixed where the difference lives

`GuestTrial` carries the database and puts it before the subcommand, where a global option goes.
`cmd_turn` passes what the caller was given. With it:

```text
two runs agree: 198 imports, fault 0xffffffffffffffff
```

which is the wall `./bin/orbistoun run` reports.

## Why it hid

The dispatcher's own documentation says it *"spawns this binary as the guest runner, which is
what worker mode already does: the runner is then literally the same build and cannot be a stale
copy"*. That is true and was the thing being guarded - **the same binary**. Nobody asked whether
it was the same *configuration*, and a symbol database is configuration that changes which code
runs.

A sweep that boots the same executable with a different symbol set is exactly the shape principle
3 names for a baseline: *a baseline taken with a stale variable set is not a baseline at all*,
which `spawn` already says about diagnostic variables three lines above where the database was
missing.

## What this does not establish

**That every earlier turn conclusion is wrong.** A conclusion about a call that is named in both
configurations is unaffected. Which of this session's turn results survive has not been
re-derived, and the honest position is that each needs re-running rather than re-reading.

**Nor that the two configurations differ only here.** The bare command also omits whatever else
the driver sets. `--symbols-db` is the one whose absence was measured; a second difference would
look the same and has not been searched for.
