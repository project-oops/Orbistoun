# 532. PPSA02664's flip-wait is a symptom; the wall is the Dcb builders (FLIP_TO_ALL confirms)

**2026-09-14** - chasing the one non-REQ-gated lead from the corpus sweep, while d3cb is pending

The corpus sweep (worklog ~) flagged PPSA02664 blocking on `"UnityFTMFlipQueue" … waited on, never
delivered`, which read like a tractable win independent of the AGC builders: post the flip completion and
the guest proceeds. `orbistoun-env` already has the diagnostic for exactly that question -
`ORBISTOUN_FLIP_TO_ALL` ("post a flip completion to every queue - would waking that wait get the guest
further?", D615/D224). This ran it.

## The experiment

`ORBISTOUN_FLIP_TO_ALL=1 ./bin/orbistoun run PPSA02664 --limit 10`. The run intervenes, so it recorded
nothing (compat untouched, D224).

**The wall did not move.** Same fault, byte-for-byte: `read of 0xa8 while executing at 0x7fff13abdc8d`
(inside `memcpy`), same access violation. `UnityFTMFlipQueue` still showed `0 delivered`. Posting flip
completions to every queue changed nothing.

## What it rules out, and what it points at

The flip-wait is **not** the wall. The recorded fault is on a different thread, and the calls
immediately before it are the **unimplemented Dcb builders**:

```
libSceAgc::sceAgcDcbWaitRegMem(...)          -> 0xf7ff0001   from image+0x43573
libSceAgc::sceAgcWaitRegMemPatchAddress(...) -> 0xf7ff0001   from image+0x435df
libSceAgc::sceAgcDcbWaitRegMem(...)          -> 0xf7ff0001   from image+0x38f52
```

The guest calls a Dcb builder, gets orbistoun's `0xf7ff0001` placeholder, and then `memcpy`s from a
struct field at offset `0xa8` of a base that is null - the field the builder's success path would have
populated. So PPSA02664's fault converges on the **AGC Dcb command builders**, the same layer
**REQ-20260913T2346Z-d3cb** asks for the writer-handle layout to implement. The flip queue is a parallel
symptom of the same unbuilt command path, not a second lever.

## Consequence

Closes the flip-wait as a distraction: there is no cheap flip-delivery win here, and `FLIP_TO_ALL`
waking the wait would only have masked the real fault. d3cb is confirmed as PPSA02664's lever - the
experiment is the "second observation of a different kind" D224 requires before an intervention that
moves (or here, fails to move) a wall is trusted. Recorded a note to that effect on the d3cb request.
No code changed; nothing committed.
