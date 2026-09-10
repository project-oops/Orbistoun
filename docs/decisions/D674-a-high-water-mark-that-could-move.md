# D674 - A high-water mark that could move backwards

**Status:** decided
**Date:** 2026-09-10

## Found by not dismissing a flake

`./bin/orbistoun check` failed on `cargo test --workspace` twice, on two different turns, while
every direct re-run of the same command passed. The first time I recorded it as "couldn't
reproduce, flagging rather than claiming it's nothing". The second time was the signal.

There was no captured output either time - my own `tail -6` on the gate had thrown the failure
detail away, which is worth noting on its own: **a pipe that trims a gate's output trims the
part you need.**

So it was found by reading rather than by reproducing.

## The race

`orbistoun-input` recognises a handle it issued by comparing against a high-water mark:

```rust
static ISSUED: AtomicU32 = AtomicU32::new(0);
ISSUED.store(handle.as_raw(), Relaxed);   // in scePadOpen, and in sceMouseOpen
fn ours(raw: u64) -> Option<Handle> { … (raw <= ISSUED.load(Relaxed)).then_some(handle) }
```

The allocator is behind a `Mutex`; the mark is not. Two threads opening at once:

| | thread A | thread B | `ISSUED` |
|---|---|---|---|
| 1 | allocates 4 | | 0 |
| 2 | | allocates 5 | 0 |
| 3 | | stores 5 | 5 |
| 4 | stores 4 | | **4** |

Handle 5 is live, above the mark, and **refused by every call that takes it** - `scePadReadState`
answers `no such pad` for a pad the guest just opened.

`fetch_max` instead of `store`. A mark that only rises cannot disagree with an allocator that
only counts up.

## Not only a test problem

The tests run in parallel, which is what made it visible - but this is the guest-facing path. Two
guest threads opening pads at once hit exactly the same window, and the failure there is a title
told its controller does not exist, intermittently, with nothing in the trace to say why.

## Tested as a property, because the interleaving cannot be

A test that spawns threads and hopes to catch the window is probabilistic, which is a poor guard:
it passes on a good day and teaches nothing. The property is deterministic and is the whole of
the defect - **recording a handle must never lower the mark** - so `remember` is a function of
its own and the test records out of order and asserts the higher handle is still recognised.

Watched to fail: reverting `fetch_max` to `store` reds it with the message it was written for.
Both copies have it, because both crates carry their own mark and a fix in one is not a fix in
the other.
