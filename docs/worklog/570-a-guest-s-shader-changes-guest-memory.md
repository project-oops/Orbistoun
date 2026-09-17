# 570. A guest's shader changes guest memory, and one span had to cover both buffers

**2026-09-15** - orbistoun-gpu-vulkan, after worklog 568

The console's pixel shader stores a canary. Nothing in this project had ever seen it land.

It lands now. A word a hundred and twenty-eight kilobytes past the vertex buffer, seeded with a
value this test invented, comes back holding **`0xbeef0001`** - the shader's own constant,
written through a translated store, at a guest address, on a GPU.

That is the first time anything here has observed a guest's shader *change* guest memory. Every
result before it was about what a shader drew.

## 1. Why it took two window changes and then a third

A window is one span, and the two things the console's frame touches are far apart: the vertex
buffer its primitive shader reads, and the canary its pixel shader writes.

| | what happened |
|---|---|
| Window anchored at zero | every fetch refused; three identical vertices drawn (worklog 561) |
| Window at the vertex buffer, default length | fetches land; the store is past the end and refused (worklog 565) |
| One span covering both | both land |

Worklog 565 recorded the middle row as **correct rather than a mystery** - a window is a span
and the canary was not in it - and left widening it as the next thing. `Window::spanning`
already existed and already refused a length that is not a power of two, so the span itself was
no work at all: sixty-five thousand words, a quarter of a megabyte, which is nothing.

## 2. The part that was not the span

**Both shaders have to be translated against the same window**, and one of them was not.

The primitive shader was translated with `translate_windowed` at the vertex buffer. The pixel
shader went through `translate_staged`, which has no window parameter and therefore gets the
default - address zero. So even with the span wide enough, the pixel shader's store was computed
against a different idea of where memory starts and was refused.

Nothing about the picture would have shown that. The frame was already correct: every pixel the
colour the vertex carried, drawn by a pair of shaders that agreed about geometry and disagreed
about memory. The window is a parameter of the test helper now, and naming it at both call sites
is what makes the disagreement impossible to reintroduce silently.

## 3. What is asserted, and what that rests on

The canary value is **measured, not assumed**. The word held a sentinel this test wrote into a
host-visible buffer; the draw ran; the word came back holding something else. There is no third
party who could have written it.

It is pinned rather than printed, for the usual reason: a change that stopped the store landing
would otherwise be a line of output nobody was watching. The test also asserts the vertex words
are *unchanged*, which is the claim that holds whatever the canary does - a window wide enough
to reach the canary must not have moved the buffer the other shader reads, and a masked index
folding differently would show up exactly there.

## 4. What this still is not

Not the console's frame. The vertices are this test's, because the oracle record captured the
command stream and the shader payload and not the vertex buffer. The record's pixel hash remains
a different claim.

And the window is still the **caller's**, not the submission's. The pipeline hardcodes the
default, so a shader translated through `Pipeline::submit` gets a window at address zero and
every guest access in it is refused. Deriving one from the command stream needs register
vocabulary nobody has measured yet - which registers name a buffer - and inventing that mapping
is exactly what the vocabulary exists to stop.

## 5. Files

- `crates/orbistoun-gpu-vulkan/tests/console_fragment.rs` - the span, the window at both call
  sites, the seeded sentinel and the canary assertion.

## Next

1. The pipeline's own window. It needs register vocabulary that names buffer addresses, which is
   measurement rather than code - an obSCEne probe question.
2. `REQ-20260914T1720Z-9c4a`, which settles the payload split D688 assumes.
