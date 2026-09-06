# 2026-09-03 - (/loop) A closed gap that both surfaces still called open

```
differential   247  ->  263 cases
tests                1998
```

Arrived by hand again - the sixteenth wakeup that did not fire.

## The thirteen data imports were already answered

The plan carried *"13 imports name data, not a function - a thunk is the wrong answer and
orbistoun has no other one"* as a gap worth a skeleton. Listing them took one command - they are
already marked `[data]`:

```text
libc  _Stdout   libc  _Stderr   libc  _ZTVSt13runtime_error   libkernel  __stack_chk_guard  …
```

The C++ runtime's stream objects, vtables and locale ids, plus the stack canary. And then D307,
which introduced the marking, turned out to carry its own correction: *"what was deliberately
not done **(done in D323)**"*.

**D323 did it.** One zeroed page per data import, and `ImportResolver` asks for it before the
thunk table. The worker's relocation summary even says so - *"imports name data and were given
storage rather than a stub"* - while `orbistoun-cli imports`, the surface anybody checks first,
still ended with "and orbistoun has no other one yet". Corrected. D510.

**Eighth stale work item this week**, and the pattern is now clear enough to name: the text
describing a problem lives somewhere other than the code that fixes it, so the description is
never revisited. Six of the eight were found by *working* the item rather than by reading the
queue, which called all six outstanding.

Also removed a doc comment duplicated line-for-line above `describe_relocations`.

## `strtok_r`, and the property these cases cannot prove

**247 -> 263 cases.** Six sequences through the reentrant form, the same inputs as `strtok`'s on
purpose - the two must agree with each other as well as with the reference.

The replay path is shared, so the D508 wire was checked first: `run_strtok_sequence` named
`"strtok"` outright, and every reentrant case would have run against the non-reentrant function.
Taken from the case instead. The `saveptr` is this test's storage, threaded through every step
of one sequence, exactly as a caller must.

**These cases cannot distinguish a `strtok_r` that delegates to `strtok`** - it would answer
every one of them correctly and still be wrong, because two interleaved sequences would share a
place. Said so in the helper rather than left to be assumed, and checked separately: orbistoun's
reads the caller's pointer when `text` is null and writes the rest back, so it is right for the
reason the cases do not reach. A test that *would* reach it needs two interleaved sequences,
which the record format's one-subject-per-case shape does not carry - the same design step the
wide-character family needs.

## State

`cargo test --workspace` green - 119 suites, **1998 tests**, 0 failures. **Differential 263
cases**, all agreeing. clippy `--tests` clean, fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-359 and D466-D510.

**Next**: `sprintf` and `vsnprintf` are the last uncovered differential functions; the wide
family and an interleaved-sequence shape both need a record-format decision first.
