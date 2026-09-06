# D550 - The oracle draws

**decided** - 2026-09-04

D549 built the plumbing: clear an attachment to a colour, copy the image out, read every pixel
back. This is the half that makes it an oracle rather than a pipe - a graphics pipeline, a draw,
and a fragment shader's colour arriving where the comparison will look for it.

Phase 6 step (b) is done.

## What it draws, and why the colours differ

The attachment is cleared to **red**; the fragment shader writes **blue**; every pixel comes back
blue.

Making them differ is the design. Had both been blue, a pipeline that never bound, a vertex
shader that produced no triangle, and a fragment shader whose output went nowhere would *all*
have passed - the clear alone produces the expected image. Red is what failure looks like, and it
is a **different answer rather than an absent one**, which is the distinction between a check and
a demonstration.

## The shaders are hand-written, and that is the point

`orbistoun_spirv::fullscreen_triangle_vertex_module` and `constant_colour_fragment_module`,
assembled instruction by instruction through the builder. **The oracle exists to check the
translator, so a shader the translator produced could not check it** - the failure would be
invisible in exactly the case that matters.

The builder is still this project's code, so the argument is not that it cannot be wrong. It is
that a builder wrong enough to matter here would have to produce *exactly the colour the test
asked for, from a different colour*, which is not a failure mode a mistake has.

Two choices inside the vertex shader worth recording:

- **A constant table, not arithmetic.** The usual fullscreen-triangle trick derives positions
  from `gl_VertexIndex` with shifts and a multiply-add. Three positions in an array indexed by
  the vertex index is one `OpAccessChain` and no arithmetic - and a wrong constant is visible by
  reading it, where a wrong shift is not. A composite constant cannot be indexed dynamically, so
  the table lives in a `Private` variable initialised with one.
- **The triangle is twice the size of the viewport** - `(-1,-1)`, `(3,-1)`, `(-1,3)`. Every pixel
  is inside it, so a corner coming back unwritten means the geometry is wrong rather than that
  the test looked in the wrong place.

## What the vocabulary needed

`orbistoun-spirv` had no `Vertex` execution model, no `Output` or `Private` storage class, no
`Position` or `VertexIndex` built-in, no `Location` decoration and no `OpConstantComposite`. All
are published SPIR-V enum values, added with the shape row the checker needs.

One correction fell out of it: `OpVariable`'s optional **initialiser** is an identifier and the
shape table did not check it, so a variable initialised with an undefined id would have passed
`check()`. The tail now starts after the storage class, which is a literal.

## Four breaks across the two steps, each landing somewhere different

| break | fails at |
|---|---|
| `DONT_CARE` instead of `LOAD_OP_CLEAR` | pixel (0, 0) |
| copy region one row short | pixel (0, 2) - the **last** row only |
| `cmd_draw` with zero vertices | every pixel, red - the clear survives |
| triangle shrunk to a quarter | pixel (3, 0), while **(0, 0) still passes** |

**A test sampling one corner would have missed two of the four.** That is the argument for
checking every pixel, made twice by different mistakes, and it is why the plumbing was built and
broken before the draw was put on top of it.

There is also a control: `drawing_nothing_still_answers_the_clear` runs the same machinery with
no shaders and expects red. Without it, a pipeline that somehow drew on every path would make the
draw test pass for the wrong reason.

## What this cannot say

**Nothing about the translator.** Nothing translated is involved. It says the harness can put a
shader's output on the screen and read it back, which is what every later comparison rests on -
and it is precisely the thing that could not be checked before, because every other part of
phase 6 is verified against material this project generated.

Nothing about interpolation, depth, blending or multiple attachments either: one constant, one
attachment, no depth buffer.

The roadmap's steps (a), (c) and (d) are now verified by something, which is what doing (b)
first was for.
