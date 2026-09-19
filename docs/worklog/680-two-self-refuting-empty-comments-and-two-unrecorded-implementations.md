# 680. Two "it is empty" comments corrected, and two implementations that had no knowledge entry

**2026-09-17** — inbox `-f746` was two self-refuting comments; fixing it ran the `orbistoun-service`
test suite, which caught two functions I had implemented earlier this session and never recorded. Both
are the same failure: a guard, or a check, believing something the code around it contradicts.

## f746: two comments that say "it is empty" above a non-empty guard

- `symbols/unaccounted-ceiling.txt:8` said "**It is currently empty**"; the file holds two names
  (`sceAudioPropagationPortalDestroy`, `sceAudioPropagationSystemDestroy`). Corrected to "it currently
  holds two names", keeping the paragraph about how it shed the 202 it once carried.
- `crates/orbistoun-service/src/symbols.rs:514` said `SERVES_NOTHING` is "**Empty** ... every declared
  library now answers at least one call"; the const on the next line has 23 entries (the README's
  generated block agrees, `149 across 23 libraries`). Corrected to state the 23, keeping the story of
  the two that retired from it (`libSceGnmDriver`, `libSceAudioOut`).

Both sit on guards - the ceiling can only shrink, `SERVES_NOTHING` fails a build in both directions - so
a reader who believes "empty" concludes there is nothing left to maintain. That is the stale-count
failure, on machinery rather than a figure.

## The two unrecorded implementations the fix surfaced

`every_implemented_function_is_written_down` (`orbistoun-service`) failed: every name in an
`implementations()` table must have a `Knowledge::get(name)` entry. Two from this session did not:

- **`sceAgcDcbWaitUntilSafeForRendering`** (wired as a measured no-op, worklog 668) had no entry in
  `libSceAgc.toml`. Added one recording obSCEne `-4e91`: the library-level no-op (GetSize symbol
  absent, 0 bytes and rc `0x0` under every writer state), `known_by = "measured"`.
- **`sceAppContentTemporaryDataMount2`** (answered `0`, worklog 673) had no entry in
  `libSceAppContent.toml`. Added one, `known_by = "assumed"` (answered by the library's context-gated
  precedent, and it does not move Terminator's `int 0x41` wall), with the out-parameter left unwritten
  recorded as an assumption.

Both were missed because each unit ran its **defining** crate's tests (`orbistoun-gpu`,
`orbistoun-systemservice`) and not `orbistoun-service`, where the completeness test lives. This is the
same shape as the prose-gate miss (worklog 664): a per-crate gate passing while a cross-crate one
fails. Recorded as a standing lesson - run `./bin/orbistoun check`, which includes both, when adding to
any `implementations()`.

## Gate state

`orbistoun-service` 65 pass, `orbistoun-hle` 65 pass, `knowledge-audit` exit 0; `clippy -p
orbistoun-service -- -D warnings` clean; `./bin/orbistoun prose` exit 0; identity scan clean; the
ceiling file still lists exactly two names (comment-only change). No commit.
