# 801. The nightly gap-analysis inbox was going unread (the loop polled the outbox); reading it cleared four stale-fact corrections this session's own changes created, and surfaced the execution-tracer as a filed request

**2026-09-22** — the operator pointed at `C:/tmp/orbistoun/worklog.md`. That is orbistoun's **inbox** -
requests *to* orbistoun from sibling projects and a nightly gap-analysis agent - and the loop had been
polling `C:/tmp/Obscene/worklog.md`, the **outbox**, for a wall-unblocking result. The inbox held a stack
of open requests, several of them defects this session's own uncommitted work introduced.

## Corrected

- **REQ-6674** - two prose counts in `symbols.rs` (`148 across 22`, `149 across 23`) went stale when
  `libSceCoredump` left `SERVES_NOTHING` (worklog 798); the generated block reads `147 across 21`. Both
  transcriptions updated to match, and the retirement narrative that names the moved-out libraries now
  includes `libSceCoredump` (four, not three).
- **REQ-3428** - the `module_start.rs` doc-comment said *"there is no `scePthreadGetaffinity` to check it
  with"*; there is now (worklog 797). Rather than restate it, the affinity test was extended to the
  set-then-get the comment said was impossible, and it pins D523: the running-thread setter drops, so the
  getter reads the creation-time value (zero for an unregistered handle), not the `0x1ffb` just set. Its
  second item - a knowledge note citing "worklog 797" that did not exist when the request was filed - is
  already satisfied, because worklog 797 was written after.
- **REQ-3e4d, REQ-df45** - `PROJECT_STATUS.md`'s two hand-written wall narratives still described three
  walls (one live), `222` imports, the `VCRUNTIME140.dll+0x1dc8d`/`0xa8` outcome and `image+0x43c4`, and
  called the threading question open. Both restated to the six-title survey (worklogs 776/796): six live
  walls with their exact sites, `224` imports, `image+0x3f8f0` traced to `array[1]+0x18`, and threading
  ruled out. `status --check` still green (the edits are in prose beside the generated block, not in it).

## Found, and left honest

- **REQ-3ac2** asked for PPSA28061's stale `[status]` record (933 calls, `image+0x43c4`, 2026-08-23)
  re-recorded to the current wall. It **cannot be, honestly**: the current 394-call run rests on the two
  `assumed` region-returns (worklog 787), so `compat record --force` correctly files it under
  `[experiment]` - "under unimplemented, with 2 functions answered by name, none of it measured, kept
  apart from the honest record" - and leaves `[status]` untouched. PPSA28061's honest `[status]` is now
  blocked by its own shipped assumptions, which is the accurate state, not a re-record the request can
  demand. The scratch `[experiment]` write was reverted.
- **REQ-7a91** files, formally, the **execution/branch tracer from entry** that worklogs 795/796 named as
  the one capability behind five of the six titles' walls - reached by computed dispatch that static RE
  cannot follow. It is the large build the loop deferred for want of a stated priority; the priority is
  now stated, and it points at the debug-register machinery already in
  `crates/orbistoun-worker/src/watchpoint.rs`. This is the next real work when it is taken up
  deliberately.

## Gate state

Code and docs changed: `symbols.rs` counts + narrative, `module_start.rs` doc and test (set-then-get),
two `PROJECT_STATUS.md` wall narratives. No records forced. `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
