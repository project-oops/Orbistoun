# D863 - An instanced primitive draw runs a workgroup per instance

**Status:** decided
**Date:** 2026-10-14
**known_by:** published

A draw of more than one instance runs its primitive shader once per instance, each instance a
mesh workgroup of its own: the dispatch is a workgroup per draw across and one per instance down,
and an instance finds itself in `WorkgroupId.y`. A seeded primitive shader (D730) receives it in
`v8`, the instance id radeonsi's primitive shaders read after the vertex id and two user VGPRs
(`si_shader_args.c` 363-371), in every vertex thread. The draw's instance count travels in its
words (`DRAW_DATA_INSTANCES_WORD`), and a batch (D718) takes only draws of one count, since one
dispatch runs every draw in it that many times.

**Why:** the geometry engine launches a draw's instances as separate subgroups, each with the same
vertices and its own instance id, so a workgroup per instance is the host's form of the same
thing. PPSA21564 draws a full-screen pass per slice of an array this way, its layer taken from
`v8` (D859), and refused before any of its frames could be drawn.

**Rejected:**
- Repeating the draw once per instance on the host: a pipeline bind and a dispatch per instance
  where one dispatch does.
- Instances as workgroups along `x` beside the draws: a batch's draws read their words by `x`
  (D718).
