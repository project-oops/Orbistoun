# 835. Each draw binds the shader in force at its own packet

**2026-09-24**: worklog 834 left Neverball's level geometry fanning into one point near the screen
centre. Working from the source first, the open-toolchain GL context's draw path (oops-sdk
`gl_draw.c`, around the vertex ring) settled two suspects before any run:

- **The window size is not it.** The vertex ring is `OOPS_GL_VBO_RING_TRIANGLES` (450) × 144 bytes,
  64,800 bytes (`gl_internal.h:3819`). That is well inside the 256 KiB window D711 places.
- **Positions are clip space with their real `w`.** The draw path culls only a triangle wholly behind
  the eye and hands the hardware `c0`–`c2` undivided, so clipping is the host's. Vulkan clips
  homogeneously.

The same code named what *was* wrong. A vertex is **48, 64 or 80 bytes**, carrying 2, 3 or 4
parameters, and each size is read by its own vertex program. The context swaps the program between
draws of one frame.

## The bug

`registers::shader_candidates` keeps, for each stage, the **last** program address the whole stream
writes. The pipeline translated that one per stage, bound it once, and ran every draw of the
submission with it. A draw made before a program swap ran the wrong program.

The run's new `last-submission.txt` showed it: the last submission had one vertex module and one pixel
module, and one bind of each.

## The fix

- `registers::shader_candidates_before(writes, vocabulary, before)` returns the program addresses in
  force at a packet.
- `Pipeline::bind_shaders`:
  - prepares the reconciled candidates as before;
  - then resolves each draw's addresses, translating any the first pass had not covered. A failure is
    recorded once, however many draws name it.
- `push_geometry_commands` emits a `BindShader` before a draw whenever the draw's module for a stage is
  not the one bound. This is the same pattern as its per-draw user data and blend.
- A stage the guest *registered* keeps its registration.
- The candidate preparation moved into `Pipeline::prepare_candidate` so both passes count and report
  alike.

Diagnostics: the run's last submission is now always written as `last-submission.txt` and
`last-module-*.spv` in the traces directory (`write_submission`, generalised from worklog 833's failed
one).

## Evidence

- Unit test `a_draw_between_two_binds_sees_the_first`: a draw between two binds sees the first
  address, after both sees the second, and before either sees none.
- Neverball, 150 s:
  - 14 frames drawn, 15 of 15 submissions to completion, 0 refused;
  - the last submission binds shaders **32 times across 3 modules**, where before it bound 2 modules
    once each.
- The cube is unchanged (`frames-confirmed: 0x5`).

## What did not move

The picture. The fans still converge near the centre (~960, 545), so this was a real bug and not
*this* bug. The next unit reads one fanning draw's own vertex data out of guest memory, computes what
the GL context wrote there (position, then parameters, at the vertex's size), and compares it with
what the translated vertex program emits. That is a measurement, not another guess.

## Also in this tick

Worklog 834's gate had failed on clippy (`items_after_statements` in the new clamp test). It was fixed
here, and this tick's gate covers both.

## Gate state

`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
