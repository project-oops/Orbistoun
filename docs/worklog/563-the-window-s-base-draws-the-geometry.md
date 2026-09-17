# 563. The window's base draws the geometry, and the wall moves to the colour parameter

**2026-09-14** - orbistoun-gpu-vulkan, measured on an NVIDIA device against another session's
**in-flight, uncommitted** working tree. Read the provenance note before trusting a number here.

## Provenance, because it changes what this is

Every measurement below was taken against a working tree another session was **actively editing** -
the guest-memory-window *base* it exercises (`Window { base }`, subtracted in `word_index` and
`address_within_window`) is that session's uncommitted work, not a committed advance. Mid-way through
this unit the tree moved under it: `orbistoun-translate` and the operand table went into an
inconsistent state and the console shader stopped translating at all (`expected exactly three
operands`). So this is a **snapshot of a transient state**, not a claim about what HEAD does - HEAD
still translates the mesh with no base. Recorded because the measurement localises a wall and the next
step is cheap, not because the state it measured still exists. No source of that session's was
changed; a probe edit made here was reverted.

## What drew, at the snapshot

`the_consoles_two_shaders_draw_together` on an RTX 5070 Ti, window anchored at the console's vertex
buffer (`base = 0x0090_0000`):

- **The isolation draw is green.** The mesh module over a *constant* fragment shader fills the
  viewport - first pixel `[0, 255, 0, 255]`. With the base, the mesh **fetches its three positions
  out of the window and exports a viewport-covering triangle**; anchored at zero it drew a degenerate
  one (worklog 561). The geometry path works when the window has a base.

## What was still wrong

- **The console's own fragment draw is black** - first pixel `[0, 0, 0, 255]` where the colour is
  seeded green `[0, 1, 0, 1]`, in-window at words 4-7 (confirmed in the read-back). Black is not the
  clear (red), so the fragment ran and wrote; the colour reaching it is zero.

## Where the fault is, as far as it was pinned

Bracketed by two tests that pass: `the_consoles_pixel_shader_draws_on_a_device` (the fragment reads
and exports attribute zero correctly, given a good vertex module), and the isolation draw (geometry
and the `store_vec4` path are sound). So the fault is the **colour parameter** the mesh hands the
fragment.

From the emitted module's disassembly: it declares two parameter outputs, `Location 0` (sourced from
vector registers v10-v13) and `Location 1` (v6-v9), for the console's `exp param0` and `exp param1`.
The fragment interpolates attribute zero, which is **Location 0 = v10-v13**. Seeding the colour slot
green and getting black back proves Location 0 does **not** carry the colour-slot value.

## The one thing this unit did not settle

Two causes remain and this could not choose between them, because the test that separates them needs a
device run the churning tree would not give:

- **(A) Linkage.** The colour is exported to `Location 1`; the fragment reads `Location 0`, which
  carries the texcoord field (seeded zero here) - so it reads black. The fix is the parameter/attribute
  mapping.
- **(B) Value.** `Location 0` *is* the colour, but the register it sources arrives zero - one of the
  console-specific differences worklog 560 catalogued (mask save/restore, export order, carrying
  address add). The fix is in the fetch or the mask.

The decisive check is one line: seed a **distinct** sentinel in the texcoord slot (words 8-11, seeded
zero today) and redraw. Texcoord's colour coming back proves (A); black persisting points at (B). It
was written and reverted when the tree stopped translating.

## Surprises

**"Give the window a base" was two walls.** The base drew the geometry and uncovered a second fault
the missing geometry had hidden - a shader that fetches nothing never reaches its colour. Visible only
because the test separates the paths with an isolation draw.

**The tree changed under the measurement.** A second session owns this subsystem and was rewriting it
live; a file changed on disk between an edit and its verification. The lesson is coordination, not
code: measuring another session's uncommitted state and filing it as an advance is the same
over-claim principle 3 forbids one level down. This entry is reframed to a snapshot for that reason.

## Next

1. **Do the sentinel check and the bisection in isolation**, not in the shared tree - a worktree at a
   settled commit, or after the owning session lands its changes. Deleting the console's mask
   save/restore, then its export reordering, is worklog 560's mechanical step, retargeted from
   geometry (done by the base) to the colour parameter.
2. The window's **length** half is still deferred and still not the blocker for this test (its buffer
   fits 64 words); it becomes load-bearing for a real frame, not this one.

## Files

- No source changed. The only artefact is the gitignored `target/spirv/console-vertex-mesh.spv` the
  existing mesh-translation test writes.
