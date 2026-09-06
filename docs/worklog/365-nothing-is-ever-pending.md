# 2026-09-03 - (/loop) Nothing is ever pending: the guest left its frame loop

```
before   161 distinct imports, 20,000,000 calls - spinning, never left the loop
after    182 distinct imports,    415,411 calls - left the loop, faults somewhere new
suites 123   tests 2005   clippy/fmt/identity clean
```

Second tick of the 20-minute cron. **The prompt it fires with is now stale** - it still
describes the `read of 0x8` wall D515 took down. Replaced at the end of this tick.

## The spin

`sceVideoOutIsFlipPending` was undeclared, so it landed on a stub answering the placeholder
`0x7fff_0001`. **It returns a count, not a verdict**, so a guest asking "are any flips
pending?" was told a hundred and thirty-four million were, and waited for them.

## The answer is zero, from this crate's own model

`orbistoun-video` already documents it above `video_out_submit_flip`: there is no presenter and
no vblank, so a flip completes as soon as it is accepted. **A queue that empties on submit is a
queue with nothing in it** - the same sentence read from the other end. Nothing invented; the
value follows from a model already written down and already driving `GetFlipStatus`.

And the guest had submitted nothing anyway: one `sceVideoOutOpen`, one `sceKernelCreateEqueue`,
one `sceVideoOutAddFlipEvent`, and **zero** `sceVideoOutSubmitFlip`. It was draining a queue it
had never put anything into.

## What it bought

Three runs, byte-identical: 182 distinct, 415,411 calls, `read of 0xa0` at `image+0x1389269`.
**415,281 of 415,411 calls now land on an implementation** - five functions and 130 calls are
all that remain on stubs.

Arity 1, the same shape as `sceVideoOutClose`; the guest passes the port `sceVideoOutOpen`
answered and the registers after it carry leftovers, two of them identical.

## Both arms broken and watched to fail

```text
answer 1 instead of 0        -> "an idle port has nothing pending"        FAILED
bad handle answers 0         -> a_bad_handle_is_refused_rather_than_reported_idle  FAILED
```

Zero is a **count**, so answering it for a handle no port owns tells a guest that a port it
does not have is idle - D125's failure one value along.

## Said in the test rather than assumed

That hardware answers zero here is **not** claimed, and the test says so: a real port has a
scanout and a flip really is pending until the next vblank. What is asserted is the model
orbistoun can honestly implement with no scanout at all.

## The equeue is next and the evidence is already in hand

```text
arg0 = 0x7400012be2b8   writable, in the guest's own mapping arena
arg1 -> "eq to wait flip"   ... and on the second call, "flip equeu"
arg2 = 0x7fff0001           <- orbistoun's own placeholder, left by an earlier stub
```

**Arity 2**, a handle written through `arg0`, a caller-supplied name the guest wrote itself.
The third register holding orbistoun's own placeholder is what establishes the arity. Recorded
in the knowledge file rather than acted on: implementing it means a real event queue *and* flip
events delivered into it, which is a subsystem rather than a return value.

Decision: [D516](../decisions/D516-nothing-is-ever-pending.md).
