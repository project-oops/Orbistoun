# D211 - The call recorder is the dispatch ring; the crate declaring one is deleted


**decided** · 2026-08-24

`orbistoun-trace` sat second in the documented dependency spine and **nothing depended on
it**. Deleted.

### What was actually in it

156 lines and three declarations: a `CallEvent` struct, a `Sink` trait with one method, and
a `CountingSink` holding two atomic counters. Nothing populated the struct, nothing
implemented the trait but the counter, and nothing called any of it.

The fields that looked like unique capability - `thread_id`, all six arguments, the return
value - were **fields on a struct nothing filled in**. Not capture that would be lost;
declarations that would need writing either way.

### Why not revive it

The recording that guests actually need got built where the calls are, in
`orbistoun-thunk`'s dispatch path: a fixed-size atomic ring drained by `orbistoun-worker`
into the `CallTrace` a run reports from.

**The `Sink` trait is the one part that cannot survive the workload.** One title makes
ninety-nine million calls through dispatch in twenty seconds. A dynamically dispatched call
per guest call is precisely the "sink that blocks a guest thread has changed the program it
observes" that principle 9 forbids - and the trait's own doc comment says so, which is the
sharpest evidence that it was written before the volume was known.

The two gaps worth closing are cheaper where the recording already is. `thread_id` and the
extra fields are more atomics in the existing ring. A file sink is a **drain** concern -
write the ring out periodically - not a per-call one, so it needs nothing on the call path
at all.

### What was kept

The three gaps, recorded against the recorder that exists rather than the crate that
declared them, in `docs/BACKLOG.md`. Deleting the crate without them would have thrown away
the only part that was worth anything: the observation that an interleaved trace with no
thread column cannot be read, and that a bounded ring loses a long run's ordering after the
first 8192 calls.

### The spine is six, not seven

`core` -> `elf` -> `nid` -> `mem` -> `hle` -> `loader`. A crate in a documented spine that
nothing depends on is worse than an unused crate elsewhere: the spine is the first
description of the workspace anybody reads, and it was claiming a tracing layer that was not
there.

