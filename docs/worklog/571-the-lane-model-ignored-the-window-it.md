# 571. The lane model ignored the window it was handed, and nothing said so

**2026-09-15** - orbistoun-translate and orbistoun-gpu, after worklog 570

Worklog 570 got the console's two shaders fetching vertices and landing a store by naming a
memory window by hand. The obvious next step was to let the submission pipeline name one too,
since it hardcoded the default and therefore refused every guest access in every shader it
translated.

That change is four lines. Writing the test for it found something else.

## 1. The test that was supposed to be easy

Translate one shader through a pipeline at two different windows and assert the modules differ.
A window's base and length are **compiled into the module** - the base is subtracted before an
index is masked, and the length *is* the mask - so two windows cannot produce the same words.

They produced the same words.

## 2. `translate_windowed` took a window and three quarters of the time ignored it

There are three fidelities. `translate_windowed` passes its window to exactly one of them:

| fidelity | what it did with the window |
|---|---|
| `Wavefront` | used it |
| `Lane` | ignored it - `predicated::translate` had no window parameter |
| `Subgroup` | ignored it - same |

So a caller could ask for a window at a guest address, get a module anchored at zero, and be
told nothing. The function's name says it takes a window; two of the three paths through it
dropped that argument on the floor.

It was invisible because of the order things were built. The window arrived for the graphics
stages (worklog 561), which are forced to wavefront fidelity - so every caller that had a reason
to pass a window was, by construction, on the one path that honoured it. A compute dispatch could
have asked and been quietly refused, and nothing in the test suite asked.

**This is the same shape as the guard faults CLAUDE.md lists**: a function reporting more than
its measurement supports. `translate_windowed` reported, by its signature, that it had placed a
window, and two thirds of the time it had not.

## 3. The fix, and why not the other one

The lane model now carries a base and a length from the window, like the wavefront model does.
Four call sites, one field, one trait method.

The alternative was to **refuse** - return an error when a non-default window reaches a fidelity
that cannot represent one. That is what `write_lane_mask` does for a lane mask, and the reasoning
there is exact: a model that cannot represent a thing must say so rather than do nothing. It was
the wrong answer here only because the lane model *can* represent a window; there was no
obstacle, just a parameter nobody had threaded through. Refusing would have made the silence
loud while leaving the capability missing.

## 4. The pipeline can be told where memory is

`Pipeline::with_window` places the window every shader that pipeline translates is built
against, and the cache key carries it. That last part is not optional: the cache is keyed on the
shader's bytes on purpose, and a key that stopped there would serve the first window's module to
the second window's draw. A module would bind, a frame would draw, and every memory access in it
would be against the wrong span.

The setter clears the cache as well, which is belt and braces rather than the mechanism - but a
cache holding entries no key will ever match again is just memory nobody can reach.

## 5. What is still not derived

The window is the **caller's**. The pipeline does not work one out from the submission, because
nothing in the register vocabulary says which register carries a buffer address, and inventing
that mapping is what the vocabulary exists to stop. `REQ-20260914T2348Z-4e71` asks obSCEne for
the one draw where the buffer address and the register writes are both visible, so it can be
solved by matching rather than assumed.

Until that lands, a caller who knows can say, and one who does not gets a window at zero and a
shader that refuses every access - which is wrong, and visibly wrong, rather than wrong and
plausible.

## 6. Files

- `crates/orbistoun-translate/src/predicated.rs` - the base and length from the window, and the
  three constructors that carry it.
- `crates/orbistoun-translate/src/lib.rs` - the two call sites that dropped it.
- `crates/orbistoun-gpu/src/pipeline.rs` - `with_window`, `window`, and the window in the cache
  key.
- `crates/orbistoun-gpu/tests/pipeline.rs` - the test that found it.

## Next

1. `REQ-20260914T2348Z-4e71`, which is what a derived window waits on.
2. `REQ-20260914T1720Z-9c4a`, which settles the payload split D688 assumes.
