# D549 - The oracle the roadmap said did not exist

**decided** - 2026-09-04

Two axes closed and no guest to run, so this tick did what thirty-four had not: read
`docs/ROADMAP.md` and `docs/BACKLOG.md`, which the project keeps precisely so that a loop does
not have to invent its own priorities.

They name the work, and they order it.

## What the roadmap already said

Phase 6's contents are built ahead of the dependency spine, and that section opens by stating
the cost: **everything in it is verified against material this project generated, so it cannot
be wrong in any way its own tests would notice.** The endpoint that fixes it is framebuffer
diffing - *"the only cheap mechanical correctness oracle this project will ever have"* - and the
same page admitted it was **"the one part of that sentence that is not built"**.

G11's own table breaks the graphics pipeline into four steps, marks three as needing no capture,
and says which to do first:

> **Do (b) first**, before anything it is meant to check… It is also the only step whose value
> does not depend on any of the others, and every later step is verified by it.

Nothing about that needed a guest, a console or a capture. It needed somebody to read the file.

## What was built

`orbistoun_gpu_vulkan::framebuffer` - the attachment half of step (b). A device-local image, a
render pass whose `LOAD_OP_CLEAR` writes a colour and whose final layout transitions the image
for transfer, a copy into a host-visible buffer, and the readback.

It runs on a real device here, an RTX 5070 Ti, and `tests/attachment.rs` checks it **against
itself**: clear to a colour, read that colour back. There is nothing else to compare against,
and that is exactly the property the rest of phase 6 lacks.

**Two colours, not one.** `(1, 0, 0, 1)` and `(0, 1, 0, 0)` differ in every channel, so between
them they pin all four - a red/alpha swap is invisible in the first, a green/blue swap in the
second. Every component is `0.0` or `1.0`, which `R8G8B8A8_UNORM` encodes exactly as `0` and
`255`, so nothing here asserts a rounding.

**Every pixel, not a sample**, and that choice earned itself immediately.

## The two breaks, and why the second one matters

- Replacing `LOAD_OP_CLEAR` with `DONT_CARE` fails at pixel `(0, 0)`: the attachment comes back
  zeroed.
- Shortening the copy region by one row fails at pixel `(0, 2)` - **the last row, and only the
  last row.** A test that probed one pixel, or a corner, would have passed while the harness
  silently dropped part of every frame it would ever be asked to compare.

Different mechanisms, different assertions, which is what D544's rule asks for. And the second
is the whole argument for building this before the draw: a wrong copy region produces *plausible
pixels* rather than an error, and a draw sitting on top of it would have been debugged in the
wrong place.

## What it is not

**No draw and no shaders.** The colour comes from the clear, so this says the plumbing carries
pixels from an attachment to the host and says nothing about whether a fragment shader's output
reaches the same place. The draw and its two hand-written shaders are the remainder of step (b),
and they now sit on plumbing that has been made to fail twice.

It is also not hardware validation - like the compute harness beside it, it checks this
project's code against this project's own request on whatever device is present.

## The lesson, which is about the loop rather than the code

Thirty-four ticks picked axes by looking at what was in front of them: the ask list, the
measured-hardware comparison, the blocker re-derivation. All real, all bounded, none of them on
the roadmap. **The project had written down what to do next, and named the one item that needed
nothing, and the loop had not opened the file.**

The instruction to prefer the last plan section is right for continuing work and wrong for
choosing it. `ROADMAP.md` and `BACKLOG.md` are where a tick with no obvious next step should
start, and this is now the second time in three ticks that reading a file the loop had never
opened beat reasoning from what it already had.
