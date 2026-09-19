# 673. `sceAppContentTemporaryDataMount2` answered, and a PPSA25872 regression to flag

**2026-09-17** — chasing PPSA25872's wall from the sweep (worklog 672). The run named an unimplemented
`libSceAppContent` stub as the upstream of its `int 0x41` trap, so I implemented it - and the run then
disproved the causation, which is worth recording as much as the fix.

## `sceAppContentTemporaryDataMount2` is now answered honestly

It was declared-only; PPSA25872 (a Unity IL2CPP title) calls it, and unimplemented it answered the
`0xf7ff0001` placeholder. `libSceAppContent` is context-gated for obSCEne (its symbols are "not
available in this context", the same gate `sceAppContentInitialize` and common-dialog hit), so
guest-observed is the ceiling: it now answers `0`, on the exact terms `initialize` and
`app_param_get_int` already set - answer honestly, and do not fabricate the `mountPoint` out-parameter
whose layout nothing has measured.

## The honest-failure correction

My first draft of the doc claimed the run "traces the trap to this exact stub" - i.e. that fixing
Mount2 would move the wall. **The re-run disproved it**: verdict `same`, the identical `int 0x41` at
`image+0x17554a3`, the identical call count. So I corrected the doc rather than leave the claim
standing. Mount2 is still worth answering (a guest calls it, a placeholder is a lie), but it is *not*
this title's blocker, and the code now says so. This is the same trap principle 3 forbids - reporting
more than was measured - caught by the rule that a guard/claim is not finished until a run has tried to
make it fail.

## What actually walls PPSA25872

The `int 0x41` is a fatal assert (obSCEne measured it fatal on retail too, `-b3c2`), reached through an
*upstream wrong value*, not a call orbistoun should implement. After Mount2, the run points one stub
further on: `libSceUserService::sceUserServiceGetAgeLevel`, also unimplemented, also answering a
placeholder. Just before the trap the guest is formatting a string (`vsnprintf`/`strcpy`/`strlen` over
a `"%s"` and an "Invalid info address."/"Hostname Look…" template) - it is building an error message and
trapping. So the wall is an assert with a chain of placeholder-returning stubs behind it, not a single
call; whacking them one at a time may or may not reach the real condition, and none is measured, so this
did not continue down that chain speculatively.

## A regression worth flagging

The run reports PPSA25872 reaching **155 imports / 321,973 calls, below its best-ever 192 / 339,539 on
2026-09-13**. The additive Mount2 change cannot cause that, so something between 2026-09-13 and now made
this title reach *less* - and its recorded fault site moved too (PROJECT_STATUS's `image+0x3b383b` vs
this run's `image+0x17554a3`). That is a real regression to investigate: a title that used to run 17,566
calls further now stops earlier. Not chased here (it spans weeks of changes), but named so it is not
invisible.

## Gate state

`cargo clippy -p orbistoun-systemservice --all-targets -- -D warnings` clean; systemservice 12 tests
pass; fmt clean; `./bin/orbistoun prose` exit 0; identity scan clean. No commit.
