# 2026-09-02 - (/loop) Four outstanding items that were never hard, and the overlapping moves

```
hardware claims       11  ->  15   (12 outstanding)
differential cases    96  -> 109   (109 rebuilt, 109 agreeing, 0 diverging)
tests               1954  -> 1955
```

## The difficulty was in my note, not in the call

Four of the outstanding measurements were `sceKernelDirectMemoryQuery` flag values, carried with
the reason *"asserting it needs a direct-memory allocation to query, which is a harness rather
than one call"* - written twice, on two entries, and repeated in the plan.

Reading the probe's own check settled it in one line:

```c
int rc = sceKernelDirectMemoryQuery(0, obs_query_flags[i], probe.buffer, OBS_LAYOUT_BUFFER);
```

**It allocates nothing.** Offset zero, the flag, a 256-byte buffer. The harness I had described
did not exist because it was never needed.

The boundary itself - 0 and 1 accepted, 2 and 4 refused with the invalid-argument code - is
already in orbistoun, in a comment citing this measurement and D398. **A comment citing a
measurement is checked by nothing.** It is four assertions now, using the same buffer size the
probe declared so the conditions match rather than resemble each other. All four agree.

That is twice in two ticks that an outstanding entry's stated reason was worse than the truth -
the mutex types were "not mapped" when the mapping was sitting in the source. Worth saying
plainly: **a reason written when something is deferred decays**, and the queue is read as though
it has not.

## Overlapping moves, which is where a naive copy corrupts

Thirteen new differential cases. The one that matters is `memmove` where source and destination
overlap: copying front-to-back is correct when the destination is *below* the source and
corrupts when it is above, because the bytes it is about to read have already been overwritten.
`memcpy` is allowed not to handle it; `memmove` must, so only `memmove` is asked.

Both directions, plus same-address, zero-length, and adjacent-but-not-overlapping. Then `memset`
including a zero length and a fill of NUL, and the unbounded `strcpy`/`strcat` into a poisoned
window - where what is watched is the terminator and what is left past it, since a shim that
copies the right characters and forgets the NUL passes every test that reads the result back as
a string.

**All thirteen agree**, so orbistoun's overlap handling is right in both directions.

## The same mistake twice in one tick, which is the part worth keeping

Inserting the new arms, I anchored on `_ => None,` followed by a doc comment - and put them in
`run_with_callback` instead of `run`. The rebuild gate caught it immediately, naming all
thirteen as unbuildable.

Then, splitting `run` for the line limit, I lifted a range that **also contained the
`strncpy`/`strncat` arm**, which rode along into the wrong function. Fixed, and it happened
again on the next edit for the same reason.

The fix that stuck was to stop anchoring on prose: locate the *function* first, then find its
`_ => None` from there. A doc comment that follows one function is not a landmark inside
another, and this is the third time in this session that replacing a block moved something that
was quietly inside it.

## State

`cargo test --workspace` green - **117 suites, 1955 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. `also hardware`: 15 claimed, 12 outstanding. `also
differential`: 109 cases matching the reference program.

Nothing committed. The day holds worklogs 292-325 and D466-D480.

**Next**: of the twelve outstanding, the loader ones need the TitleOwn loader - the biggest
remaining item, PPSA02664's real wall via `il2cpp_init`, and worth more than any further
differential case. The `_s` bounded forms are worth a look first only to confirm what glibc
actually provides, since Annex K is optional and largely absent there, and a case the checker
cannot rebuild is reported rather than skipped.
