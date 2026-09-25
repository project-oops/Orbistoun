# D716 - a fragment module simulates one lane, and a known exec mask emits only the lanes it runs

**Status:** decided
**Date:** 2026-09-24

## The question

A graphics-stage module is translated at the wavefront fidelity: one invocation simulates every lane
of the guest's wave, each vector register an array indexed by lane, each write a select against the
execution mask. For a Neverball pixel shader that was 18,880 SPIR-V instructions, and every pixel ran
all of it. For its primitive shader it was 37-49k, and the GL context draws one triangle per draw,
three vertex lanes of 64. The GPU was busy 800 ms of every second. How many lanes does a module
actually need?

## The choice

**At the fragment stage, one.** A fragment invocation is one pixel. The host rasteriser decides
coverage and runs the quad, the interpolated inputs are this pixel's, and the export already read
lane zero. The other lanes read no input and wrote no output. What lane zero computes is unchanged,
because no instruction the model translates reads another lane's registers. A branch that asks
whether any lane survives the mask asks it of the lanes the module has, so an entry mask or a
whole-quad expansion with bits for lanes that do not exist cannot keep a loop running.

**At every stage, a lane the mask is known to exclude emits nothing.** `s_mov_b32 exec_lo, <constant>`
leaves the mask known until the block ends: `control` resets it at every block, because a block can
be reached from more than one place. A lane whose bit is known clear emits no write. A lane whose bit
is known set writes without the select and without loading the old value. This is the hardware's own
behaviour, not an approximation of it. Instructions that write a mask (comparisons, carries,
`v_div_scale`) still compute every lane, so their answer for an inactive lane is exactly what it was.

**Each stage runs at the wave width the stream declares:** `VGT_SHADER_STAGES_EN.GS_W32_EN` for the
primitive shader, `SPI_PS_IN_CONTROL.PS_W32_EN` for the pixel shader (`gfx103.json`). The instruction
stream cannot say which it was compiled for (D141), and the live pipeline translated everything as 64
lanes. A GL wave32 primitive shader then simulated 32 lanes that do not exist.

## Why not the subgroup model

One invocation per lane, with masks built by subgroup ballot, is the general answer for the mesh
stage (`Fidelity::Subgroup`). It is exact where the host's subgroup matches the guest's wave, which
it does on this machine's device (32). It is also much larger: every lane-dependent method becomes
dynamic, and the local data share becomes genuinely shared. These three changes are exact, local,
and together took Neverball's GPU time from ~820 to ~100 ms/s. The subgroup model remains the answer
for a primitive shader whose mask the translator cannot know, such as a compiler-generated one that
narrows `exec` from a comparison.

## Consequences

- A fragment module can no longer be diffed lane-for-lane against the per-lane model beyond lane
  zero. Nothing did, and lane zero is still the whole of its result.
- `Model::lane_may_run` and `Model::enter_block` are the seam: a model that knows nothing answers
  `true` and ignores block boundaries, which is the previous behaviour exactly.
