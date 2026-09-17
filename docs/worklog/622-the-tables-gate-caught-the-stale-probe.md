# 622. The tables gate caught the stale probe already and said the wrong thing about it

**2026-09-16** - orbistoun-gen, `bin/orbistoun`, the finding filed as `REQ-20260915T0929Z-4c2f`

The finding said the `tables` gate checks a table against its recording but never the recording
against its probes, so a probe edited without re-recording would pass. That was the right hazard
and the wrong diagnosis. The comparison **was** already there: every solver calls
`assembler::check_recording` before replaying, and it compares the recorded `.in` against the probe
text being solved now. What was missing is that nobody could tell, because the gate threw the
answer away.

## What it looked like before

A probe line added to `probes/memory.s` without re-recording, and then `./bin/orbistoun tables`:

```
  ok       crates/orbistoun-shader/data/buffer-formats.toml
[warn] operands could not replay - recordings may be stale
[fail] a generated table no longer matches its generator - it was edited by hand, or the generator changed
```

Three things wrong with that, in rising order of cost. It is styled `warn` for a condition that
fails the run. It says "may be stale" about something the tool had just *determined*. And the `fail`
line names two causes - a hand-edited table, a changed generator - and **neither of them is what
happened**, so it sends the reader to the two files they did not touch and away from the one they
did.

The cause was one redirection: `2>/dev/null` on the generator call. `check_recording` had already
produced the sentence that mattered, naming the recording and saying what to do, and the gate
discarded it and then guessed.

## What it says now

```
  a probe file has been edited since its recording was taken:
    Error: the recording at crates/orbistoun-gen/tests/fixtures/transcripts\operands-memory.in
    was taken for different probes than the ones being solved now - re-record it with --record,
    on a machine that has llvm-mc
[fail] a recording no longer describes the probes it was taken from - re-record it in the build VM
```

Two outcomes told apart: a stale recording, and a replay that failed for any other reason. Each
prints what the generator actually said rather than a guess, which is the rule about a message
naming a cause it did not determine, applied to a gate instead of to a diagnostic.

## Watched rejecting, both ends

The acceptance asked for the gate to be seen failing, so it was: a probe added to `memory.s`, the
run above, then reverted and checked byte-identical against a saved copy, gate green again.

That is a thing somebody did once, so five tests hold it. `check_recording` had **no** test at all
before this. A recording taken for these probes is accepted; a probe added since is refused; a probe
removed is refused too; a missing recording names itself; and line endings alone are **not** a probe
edit - that last one matters because these files are checked out on Windows half the time, and a
gate that failed on a clean tree would teach everyone to stop reading it.

Both refusal tests were made to fail, by making the guard's condition always true - the exact
regression of returning it to the no-op it used to be.

## The coupling that would have rotted

The gate tells the two outcomes apart with `grep -q "was taken for different probes"` against the
generator's own message. That is a string duplicated across a language boundary, and the shell
cannot call into the crate to get it. Reword the Rust message and the `grep` stops matching with no
failure anywhere: the gate silently returns to blaming a hand-edited table.

So it is pinned from both ends. The test module holds the phrase as `STALE_MARKER` and asserts the
error carries it; `the_gate_looks_for_the_words_this_message_carries` reads `bin/orbistoun` off disk
and asserts the script still greps for it. Changing either side alone fails. That one was made to
fail too, by rewording the script's grep.

## A comment that claimed the check

`read_recording`'s doc said the input was "recorded alongside the output and **checked**, not
ignored". The function body is `let _ = input_path;`. The check lives one function away, in
`check_recording`, and the comment describing the hazard sat directly above the code that does not
address it - which is a fair part of why the finding was filed believing no check existed. Rewritten
to say what the function does and where the comparison actually is.

## Files

- `bin/orbistoun` - the `tables` step: stderr kept, the two outcomes separated, the closing message
  no longer naming a cause it has not determined.
- `crates/orbistoun-gen/src/assembler.rs` - five tests, the marker constant, the corrected comment.
- `crates/orbistoun-gen/Cargo.toml` - `tempfile` as a dev-dependency, for a directory to write a
  recording into.

## Next

`4c2f` is done. The remaining shader-side findings both wait on measurements now filed with obSCEne:
the 10/11-bit packed floats and the packed-store rules (`REQ-20260916T1250Z-b3d4`), and the capture
that four roadmap items sit behind (`REQ-20260916T1250Z-a1f7`).
