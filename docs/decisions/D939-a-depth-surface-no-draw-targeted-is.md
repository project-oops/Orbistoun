# D939 - A depth surface no draw targeted is read from guest memory

**Status:** decided
**Date:** 2026-10-21
**known_by:** assumed

A draw may sample a flat depth surface (a depth swizzle and a depth image format, D817) that no
draw in the run has yet bound as its depth target, nor cleared through one. The console's sampler
reads whatever its memory holds. Here it is read the same way: one level of `16_UNORM` or
`32_FLOAT`, detiled by the swizzle equation for its mode (D923) and bound as `f32` depths, compared
through the sampler where the module compares. A surface a draw has targeted is the backend's
attachment, as before (D817), and a cleared one its clear value (D935).

**Why:** Grand Theft Auto V's loading-screen lighting samples a 512x512 `16_UNORM` shadow map per
light beside the 1024x1536 one it draws. Nothing in the run draws the smaller ones before that
pass, and the backend refused the segment for binding a depth target it never drew. Every later
submission waited behind it.

**Rejected:**
- Binding depth one, nothing shadowed: it answers for what memory holds with a guess.
