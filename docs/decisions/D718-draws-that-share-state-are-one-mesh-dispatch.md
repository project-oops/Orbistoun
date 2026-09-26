# D718 - Draws that share state are one mesh dispatch

**Status:** decided
**Date:** 2026-09-25

A run of consecutive guest draws that differ only in the geometry stage's user data is one host
mesh dispatch with one workgroup per draw; each workgroup reads its user data from a storage
buffer at binding 5, at `WorkgroupId.x * 16`. Any other command recorded into the pass records
the open batch first.

**Why:** the batch key covers the pipeline, attachment, window and fragment words, so only user
data differs and each workgroup reads exactly its draw's words. Vulkan's mesh primitive ordering
passes a workgroup's primitives on before any later workgroup's, so rasterisation and blending
keep the draws' order. A GL guest issues one draw per triangle, and a dispatch per draw costs
host time per triangle.

**Rejected:**
- One dispatch per draw: host work per triangle.
- Push constants for user data: shared by every workgroup in a dispatch.
