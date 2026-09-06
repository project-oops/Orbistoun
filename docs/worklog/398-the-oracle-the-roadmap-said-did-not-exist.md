# 2026-09-04 - (/loop) The oracle the roadmap said did not exist

```
framebuffer attachment path built and verified on a real device (RTX 5070 Ti)
clear -> copy -> read back, every pixel; broken twice, two different assertions
suites 132   clippy/fmt/identity clean on both repos
```

Thirty-fifth cron tick. No guest binary, two axes closed - so this tick finally read
`docs/ROADMAP.md` and `docs/BACKLOG.md`, which thirty-four ticks had never opened.

## The roadmap already named the work and ordered it

Phase 6's contents are built ahead of the spine, and that page states the cost plainly:
**everything in it is verified against material this project generated, so it cannot be wrong in
any way its own tests would notice.** The fix is framebuffer diffing - *"the only cheap
mechanical correctness oracle this project will ever have"* - which the same page admitted was
*"the one part of that sentence that is not built"*.

G11's table marks three of four steps as needing no capture and says which comes first:
**"Do (b) first, before anything it is meant to check… every later step is verified by it."**
No guest, no console, no capture. It needed somebody to read the file.

## Built

`orbistoun_gpu_vulkan::framebuffer` - the attachment half. A device-local image, a render pass
whose `LOAD_OP_CLEAR` writes the colour and whose final layout transitions the image for
transfer, a copy into a host-visible buffer, the readback.

`tests/attachment.rs` checks it **against itself**: clear to a colour, read that colour back.
Two colours, not one - `(1,0,0,1)` and `(0,1,0,0)` differ in every channel, so between them they
pin all four. Every component is `0.0` or `1.0`, exact in `R8G8B8A8_UNORM`, so no rounding is
being asserted. **Every pixel, not a sample.**

## The two breaks

`DONT_CARE` instead of `CLEAR` fails at pixel `(0,0)`. A copy region one row short fails at
`(0,2)` - **the last row and only the last row**, which a probe of one pixel or a corner would
have passed while the harness silently dropped part of every frame.

Different mechanisms, different assertions (D544's rule), and the second is the argument for
building this before the draw: a wrong copy region produces plausible pixels rather than an
error, and a draw on top of it would have been debugged in the wrong place.

## Not built

**No draw and no shaders.** The colour comes from the clear, so this says the plumbing carries
pixels and says nothing about a fragment shader's output reaching the same place. That is the
remainder of step (b), and it now sits on plumbing made to fail twice.

The roadmap entries that called this unbuilt are rewritten rather than left (check 13).

## The lesson is about the loop

Thirty-four ticks chose axes from what was in front of them. **The project had written down what
to do next, and named the one item needing nothing, and the loop had not opened the file.**
Second time in three ticks that reading something never opened beat reasoning from what was
already to hand.

Decision: [D549](../decisions/D549-the-oracle-the-roadmap-said-did-not-exist.md).
