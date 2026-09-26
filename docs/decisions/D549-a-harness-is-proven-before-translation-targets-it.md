# D549 - A harness is proven before translation targets it

**Status:** decided
**Date:** 2026-09-04

Each host graphics stage the translator will target is first built and checked with hand-written
shaders against its own framebuffer readback: attachment and copy, then a draw, then each input a
stage consumes. Only then is translated output compared against it, pixel for pixel. The
reference draws a colour that differs from the clear, and every pixel is checked.

**Why:** everything else in the graphics path is verified against material this project
generated, so it cannot be wrong in a way its own tests notice. A shader the translator produced
cannot check the translator. A matching clear and draw colour passes with no pipeline at all, and
a partial copy region or shrunk triangle passes a sampled corner.

**Rejected:**
- Translating first and debugging through the harness: faults land in the wrong layer.
- Using translated shaders as the reference: invisible in exactly the case that matters.
- Sampling a pixel or a corner: missed two of four deliberate breaks.
